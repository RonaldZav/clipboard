use crate::types::ClipboardItem;
use crate::input_utils::InputUtils;
use crate::ui::draw_ui;

use std::sync::{Arc, Mutex, mpsc::Receiver};
use std::collections::HashMap;
use std::time::Instant;
use std::ptr::NonNull;

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_keyboard, delegate_output, delegate_xdg_shell,
    delegate_xdg_window,
    delegate_pointer, delegate_registry, delegate_seat,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyboardHandler, KeyEvent, Keysym, Modifiers as SeatModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        xdg::{window::{Window, WindowConfigure, WindowDecorations, WindowHandler}, XdgShell},
    },
};

use wayland_client::{
    globals::registry_queue_init,
    protocol::{
        wl_keyboard::WlKeyboard,
        wl_output::WlOutput,
        wl_pointer::WlPointer,
        wl_seat::WlSeat,
        wl_surface::WlSurface,
    },
    Connection, Proxy, QueueHandle,
};

use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};

use egui;
use egui_wgpu;
use wgpu;

// ─── egui input state tracker ────────────────────────────────────────────────

struct EguiInputState {
    modifiers: egui::Modifiers,
    events: Vec<egui::Event>,
    scroll_delta: egui::Vec2,
}

impl EguiInputState {
    fn new() -> Self {
        Self {
            modifiers: egui::Modifiers::default(),
            events: Vec::new(),
            scroll_delta: egui::Vec2::ZERO,
        }
    }

    fn take_raw_input(&mut self, screen_w: u32, screen_h: u32) -> egui::RawInput {
        let events = std::mem::take(&mut self.events);
        let scroll = std::mem::replace(&mut self.scroll_delta, egui::Vec2::ZERO);
        let mut raw = egui::RawInput::default();
        raw.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(screen_w as f32, screen_h as f32),
        ));
        raw.events = events;
        if scroll != egui::Vec2::ZERO {
            raw.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: scroll,
                modifiers: self.modifiers,
            });
        }
        raw
    }
}

// ─── AppState ─────────────────────────────────────────────────────────────────

pub struct AppState {
    // SCTK protocol state
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor_state: CompositorState,
    #[allow(dead_code)]
    xdg_shell: XdgShell,
    window: Option<Window>,

    // Input devices
    keyboard: Option<WlKeyboard>,
    pointer: Option<WlPointer>,

    // Pointer position on the full-screen surface (surface-local coords)
    pointer_pos: (f64, f64),

    // App visibility / lifecycle
    visible: bool,
    shown_at: Instant,
    configured: bool,
    width: u32,
    height: u32,

    // wgpu objects (Option so we can initialise lazily after first configure)
    wgpu_device: Option<wgpu::Device>,
    wgpu_queue: Option<wgpu::Queue>,
    wgpu_surface: Option<wgpu::Surface<'static>>,
    surface_format: Option<wgpu::TextureFormat>,

    // egui objects
    egui_ctx: egui::Context,
    egui_renderer: Option<egui_wgpu::Renderer>,
    egui_state: EguiInputState,

    // Popup origin (where we should draw the egui Window)
    popup_origin: egui::Pos2,

    // App data
    history: Arc<Mutex<Vec<ClipboardItem>>>,
    show_rx: Receiver<()>,
    stop_rx: Receiver<()>,
    texture_cache: HashMap<usize, egui::TextureHandle>,

    display_ptr: *mut std::ffi::c_void,
    surface_ptr: Option<*mut std::ffi::c_void>,
}

// Safety: AppState is only ever accessed from the single event-loop thread.
unsafe impl Send for AppState {}

impl AppState {
    fn create_window(&mut self, qh: &QueueHandle<Self>) {
        if self.window.is_some() {
            return;
        }

        let surface = self.compositor_state.create_surface(qh);
        self.surface_ptr = Some(surface.id().as_ptr() as *mut std::ffi::c_void);

        let window = self
            .xdg_shell
            .create_window(surface, WindowDecorations::RequestServer, qh);
        window.set_title("Clipboard");
        window.set_app_id("io.github.ronaldzav.clipboard");
        window.set_min_size(Some((350, 450)));
        window.commit();

        self.width = 350;
        self.height = 450;
        self.configured = false;
        self.wgpu_device = None;
        self.wgpu_queue = None;
        self.wgpu_surface = None;
        self.egui_renderer = None;
        self.texture_cache.clear();

        self.window = Some(window);
    }

    fn set_input_region_empty(&self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let wl_surface = window.wl_surface();
        // An empty region means no input hits the surface → clicks pass through
        if let Ok(region) = Region::new(&self.compositor_state) {
            // Don't call region.add(…) — leave it empty
            wl_surface.set_input_region(Some(region.wl_region()));
            wl_surface.commit();
        }
    }

    fn set_input_region_full(&self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let wl_surface = window.wl_surface();
        // No region = compositor defaults to full surface receiving input
        wl_surface.set_input_region(None);
        wl_surface.commit();
    }

    fn show(&mut self, qh: &QueueHandle<Self>) {
        self.create_window(qh);
        self.visible = true;
        self.shown_at = Instant::now();
        let (mouse_x, mouse_y) = InputUtils::get_mouse_position();
        self.pointer_pos = (mouse_x as f64, mouse_y as f64);
        // Record where the popup should appear
        self.popup_origin = egui::pos2(
            self.pointer_pos.0 as f32,
            self.pointer_pos.1 as f32,
        );
        // Clamp so popup doesn't go off-screen
        let max_x = (self.width as f32 - 350.0).max(0.0);
        let max_y = (self.height as f32 - 450.0).max(0.0);
        self.popup_origin.x = self.popup_origin.x.min(max_x);
        self.popup_origin.y = self.popup_origin.y.min(max_y);

        self.set_input_region_full();
        self.render_frame();
    }

    fn hide(&mut self) {
        self.visible = false;
        self.window.take();
        self.wgpu_surface = None;
        self.wgpu_device = None;
        self.wgpu_queue = None;
        self.egui_renderer = None;
        self.surface_format = None;
        self.configured = false;
        self.surface_ptr = None;
        self.texture_cache.clear();
        self.set_input_region_empty();
    }

    fn init_wgpu(&mut self) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            ..Default::default()
        });

        let raw_display = RawDisplayHandle::Wayland(
            WaylandDisplayHandle::new(NonNull::new(self.display_ptr).unwrap()),
        );
        let Some(surface_ptr) = self.surface_ptr else {
            return;
        };
        let raw_window = RawWindowHandle::Wayland(
            WaylandWindowHandle::new(NonNull::new(surface_ptr).unwrap()),
        );

        // Safety: the display and surface pointers are valid for the lifetime of the
        // Wayland connection and wl_surface, both of which outlive this struct.
        let wgpu_surface = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: raw_display,
                    raw_window_handle: raw_window,
                })
                .expect("Failed to create wgpu surface")
        };

        let (adapter, device, queue) = pollster::block_on(async {
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    compatible_surface: Some(&wgpu_surface),
                    force_fallback_adapter: false,
                })
                .await
                .expect("No suitable wgpu adapter found");

            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("clipboard-device"),
                        required_features: wgpu::Features::empty(),
                        required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                    },
                    None,
                )
                .await
                .expect("Failed to create wgpu device");

            (adapter, device, queue)
        });

        let caps = wgpu_surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| *f == wgpu::TextureFormat::Bgra8UnormSrgb)
            .unwrap_or(caps.formats[0]);

        // Choose a composite alpha mode that supports transparency if available
        let alpha_mode = caps
            .alpha_modes
            .iter()
            .copied()
            .find(|m| {
                *m == wgpu::CompositeAlphaMode::PreMultiplied
                    || *m == wgpu::CompositeAlphaMode::PostMultiplied
            })
            .unwrap_or(wgpu::CompositeAlphaMode::Opaque);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: self.width.max(1),
            height: self.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        wgpu_surface.configure(&device, &config);

        let renderer = egui_wgpu::Renderer::new(&device, format, None, 1);

        self.wgpu_device = Some(device);
        self.wgpu_queue = Some(queue);
        self.wgpu_surface = Some(wgpu_surface);
        self.surface_format = Some(format);
        self.egui_renderer = Some(renderer);
    }

    fn reconfigure_surface(&self) {
        if let (Some(surface), Some(device)) = (&self.wgpu_surface, &self.wgpu_device) {
            let format = self.surface_format.unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb);
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width: self.width.max(1),
                height: self.height.max(1),
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };
            surface.configure(device, &config);
        }
    }

    fn render_frame(&mut self) {
        let (Some(_device), Some(_queue), Some(surface)) = (
            self.wgpu_device.as_ref(),
            self.wgpu_queue.as_ref(),
            self.wgpu_surface.as_ref(),
        ) else {
            return;
        };

        let output_frame = match surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Outdated) => {
                return;
            }
            Err(e) => {
                eprintln!("Surface error: {:?}", e);
                return;
            }
        };

        let view = output_frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // --- egui pass ---
        let raw_input = self.egui_state.take_raw_input(self.width, self.height);
        let popup_origin = self.popup_origin;
        let history_arc = self.history.clone();
        let texture_cache = &mut self.texture_cache as *mut HashMap<usize, egui::TextureHandle>;

        let full_output = if self.visible {
            self.egui_ctx.run(raw_input, |ctx| {
                // Safety: texture_cache is only accessed here and nowhere else during this closure
                let tc = unsafe { &mut *texture_cache };
                let mut history = history_arc.lock().unwrap();
                draw_ui(ctx, &mut *history, popup_origin, tc);
            })
        } else {
            self.egui_ctx.run(raw_input, |_| {})
        };

        let renderer = match self.egui_renderer.as_mut() {
            Some(r) => r,
            None => return,
        };

        let device = self.wgpu_device.as_ref().unwrap();
        let queue = self.wgpu_queue.as_ref().unwrap();

        // Textures
        for (id, delta) in &full_output.textures_delta.set {
            renderer.update_texture(device, queue, *id, delta);
        }
        for id in &full_output.textures_delta.free {
            renderer.free_texture(id);
        }

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.width, self.height],
            pixels_per_point: 1.0,
        };

        let primitives = self.egui_ctx.tessellate(full_output.shapes, 1.0);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("egui-encoder"),
        });

        renderer.update_buffers(device, queue, &mut encoder, &primitives, &screen_descriptor);

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui-render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            renderer.render(&mut render_pass, &primitives, &screen_descriptor);
        }

        queue.submit(std::iter::once(encoder.finish()));
        output_frame.present();
    }
}

// ─── SCTK delegate implementations ───────────────────────────────────────────

impl CompositorHandler for AppState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _new_transform: wayland_client::protocol::wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _time: u32,
    ) {
    }
}

impl OutputHandler for AppState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: WlOutput,
    ) {
    }
}

impl WindowHandler for AppState {
    fn request_close(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _window: &Window) {
        self.hide();
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        if let Some(w) = configure.new_size.0 {
            self.width = w.get();
        }
        if let Some(h) = configure.new_size.1 {
            self.height = h.get();
        }

        if !self.configured {
            self.configured = true;
            self.init_wgpu();
            // Start hidden
            self.set_input_region_empty();
        } else {
            self.reconfigure_surface();
        }

        if self.visible {
            self.render_frame();
        }
    }
}

impl SeatHandler for AppState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            let kb = self.seat_state.get_keyboard(qh, &seat, None).unwrap();
            self.keyboard = Some(kb);
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            let ptr = self.seat_state.get_pointer(qh, &seat).unwrap();
            self.pointer = Some(ptr);
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(kb) = self.keyboard.take() {
                kb.release();
            }
        }
        if capability == Capability::Pointer {
            if let Some(ptr) = self.pointer.take() {
                ptr.release();
            }
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: WlSeat) {}
}

impl KeyboardHandler for AppState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        _surface: &WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
        self.egui_state.events.push(egui::Event::WindowFocused(true));
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        _surface: &WlSurface,
        _serial: u32,
    ) {
        self.egui_state.events.push(egui::Event::WindowFocused(false));
        if self.visible {
            self.hide();
        }
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        // Hide on Escape
        if event.keysym == Keysym::Escape && self.visible {
            self.hide();
            return;
        }
        if let Some(key) = keysym_to_egui(event.keysym) {
            self.egui_state.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: self.egui_state.modifiers,
            });
        }
        if let Some(text) = event.utf8 {
            if text.chars().all(|c| !c.is_control()) {
                self.egui_state.events.push(egui::Event::Text(text));
            }
        }
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        if let Some(key) = keysym_to_egui(event.keysym) {
            self.egui_state.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: self.egui_state.modifiers,
            });
        }
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        _serial: u32,
        modifiers: SeatModifiers,
    ) {
        self.egui_state.modifiers = egui::Modifiers {
            alt: modifiers.alt,
            ctrl: modifiers.ctrl,
            shift: modifiers.shift,
            mac_cmd: false,
            command: modifiers.ctrl,
        };
    }
}

impl PointerHandler for AppState {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            match event.kind {
                PointerEventKind::Enter { .. } => {}
                PointerEventKind::Leave { .. } => {
                    self.egui_state.events.push(egui::Event::PointerGone);
                }
                PointerEventKind::Motion { .. } => {
                    let (x, y) = (event.position.0, event.position.1);
                    self.pointer_pos = (x, y);
                    self.egui_state.events.push(egui::Event::PointerMoved(
                        egui::pos2(x as f32, y as f32),
                    ));
                }
                PointerEventKind::Press { button, .. } => {
                    if let Some(btn) = wayland_button_to_egui(button) {
                        self.egui_state.events.push(egui::Event::PointerButton {
                            pos: egui::pos2(event.position.0 as f32, event.position.1 as f32),
                            button: btn,
                            pressed: true,
                            modifiers: self.egui_state.modifiers,
                        });
                    }
                }
                PointerEventKind::Release { button, .. } => {
                    if let Some(btn) = wayland_button_to_egui(button) {
                        self.egui_state.events.push(egui::Event::PointerButton {
                            pos: egui::pos2(event.position.0 as f32, event.position.1 as f32),
                            button: btn,
                            pressed: false,
                            modifiers: self.egui_state.modifiers,
                        });
                    }
                    // Hide when clicking outside the popup
                    let pos = egui::pos2(event.position.0 as f32, event.position.1 as f32);
                    if self.visible {
                        let popup_rect = egui::Rect::from_min_size(
                            self.popup_origin,
                            egui::vec2(350.0, 450.0),
                        );
                        if !popup_rect.contains(pos) {
                            self.hide();
                        }
                    }
                }
                PointerEventKind::Axis {
                    horizontal,
                    vertical,
                    ..
                } => {
                    self.egui_state.scroll_delta +=
                        egui::vec2(horizontal.absolute as f32, vertical.absolute as f32);
                }
            }
        }
    }
}

impl ProvidesRegistryState for AppState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

// Delegate macros
delegate_compositor!(AppState);
delegate_output!(AppState);
delegate_seat!(AppState);
delegate_keyboard!(AppState);
delegate_pointer!(AppState);
delegate_xdg_shell!(AppState);
delegate_xdg_window!(AppState);
delegate_registry!(AppState);

// ─── Key translation helpers ──────────────────────────────────────────────────

fn keysym_to_egui(sym: Keysym) -> Option<egui::Key> {
    match sym {
        Keysym::Return | Keysym::KP_Enter => Some(egui::Key::Enter),
        Keysym::Escape => Some(egui::Key::Escape),
        Keysym::Tab => Some(egui::Key::Tab),
        Keysym::BackSpace => Some(egui::Key::Backspace),
        Keysym::Delete => Some(egui::Key::Delete),
        Keysym::Left => Some(egui::Key::ArrowLeft),
        Keysym::Right => Some(egui::Key::ArrowRight),
        Keysym::Up => Some(egui::Key::ArrowUp),
        Keysym::Down => Some(egui::Key::ArrowDown),
        Keysym::Home => Some(egui::Key::Home),
        Keysym::End => Some(egui::Key::End),
        Keysym::Page_Up => Some(egui::Key::PageUp),
        Keysym::Page_Down => Some(egui::Key::PageDown),
        Keysym::space => Some(egui::Key::Space),
        Keysym::a | Keysym::A => Some(egui::Key::A),
        Keysym::b | Keysym::B => Some(egui::Key::B),
        Keysym::c | Keysym::C => Some(egui::Key::C),
        Keysym::d | Keysym::D => Some(egui::Key::D),
        Keysym::e | Keysym::E => Some(egui::Key::E),
        Keysym::f | Keysym::F => Some(egui::Key::F),
        Keysym::g | Keysym::G => Some(egui::Key::G),
        Keysym::h | Keysym::H => Some(egui::Key::H),
        Keysym::i | Keysym::I => Some(egui::Key::I),
        Keysym::j | Keysym::J => Some(egui::Key::J),
        Keysym::k | Keysym::K => Some(egui::Key::K),
        Keysym::l | Keysym::L => Some(egui::Key::L),
        Keysym::m | Keysym::M => Some(egui::Key::M),
        Keysym::n | Keysym::N => Some(egui::Key::N),
        Keysym::o | Keysym::O => Some(egui::Key::O),
        Keysym::p | Keysym::P => Some(egui::Key::P),
        Keysym::q | Keysym::Q => Some(egui::Key::Q),
        Keysym::r | Keysym::R => Some(egui::Key::R),
        Keysym::s | Keysym::S => Some(egui::Key::S),
        Keysym::t | Keysym::T => Some(egui::Key::T),
        Keysym::u | Keysym::U => Some(egui::Key::U),
        Keysym::v | Keysym::V => Some(egui::Key::V),
        Keysym::w | Keysym::W => Some(egui::Key::W),
        Keysym::x | Keysym::X => Some(egui::Key::X),
        Keysym::y | Keysym::Y => Some(egui::Key::Y),
        Keysym::z | Keysym::Z => Some(egui::Key::Z),
        _ => None,
    }
}

fn wayland_button_to_egui(button: u32) -> Option<egui::PointerButton> {
    // Linux input event codes
    match button {
        0x110 => Some(egui::PointerButton::Primary),  // BTN_LEFT
        0x111 => Some(egui::PointerButton::Secondary), // BTN_RIGHT
        0x112 => Some(egui::PointerButton::Middle),    // BTN_MIDDLE
        _ => None,
    }
}

// ─── Public entry point ───────────────────────────────────────────────────────

pub fn run(
    history: Arc<Mutex<Vec<ClipboardItem>>>,
    show_rx: Receiver<()>,
    stop_rx: Receiver<()>,
    start_hidden: bool,
) {
    let conn = match Connection::connect_to_env() {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("Failed to connect to Wayland display: {e}");
            eprintln!("This clipboard manager requires a Wayland session.");
            return;
        }
    };

    // Needed for wgpu surface creation on Wayland when using raw handles.
    let display_ptr = conn.backend().display_ptr() as *mut std::ffi::c_void;

    let (globals, mut event_queue) =
        registry_queue_init::<AppState>(&conn).expect("Failed to init registry queue");
    let qh = event_queue.handle();

    let compositor_state =
        CompositorState::bind(&globals, &qh).expect("wl_compositor not available");
    let xdg_shell = match XdgShell::bind(&globals, &qh) {
        Ok(xdg_shell) => xdg_shell,
        Err(_) => {
            eprintln!("xdg_wm_base not available");
            eprintln!("This clipboard manager requires a compositor with xdg-shell support.");
            return;
        }
    };
    let seat_state = SeatState::new(&globals, &qh);
    let output_state = OutputState::new(&globals, &qh);
    let registry_state = RegistryState::new(&globals);

    let egui_ctx = egui::Context::default();

    let mut state = AppState {
        registry_state,
        seat_state,
        output_state,
        compositor_state,
        xdg_shell,
        window: None,
        keyboard: None,
        pointer: None,
        pointer_pos: {
            let (mouse_x, mouse_y) = InputUtils::get_mouse_position();
            (mouse_x as f64, mouse_y as f64)
        },
        visible: false,
        shown_at: Instant::now(),
        configured: false,
        width: 350,
        height: 450,
        wgpu_device: None,
        wgpu_queue: None,
        wgpu_surface: None,
        surface_format: None,
        egui_ctx,
        egui_renderer: None,
        egui_state: EguiInputState::new(),
        popup_origin: egui::Pos2::ZERO,
        history,
        show_rx,
        stop_rx,
        texture_cache: HashMap::new(),
        display_ptr,
        surface_ptr: None,
    };

    if !start_hidden {
        state.show(&qh);
    }

    // Initial roundtrip to receive the configure event (and trigger init_wgpu)
    event_queue.roundtrip(&mut state).unwrap();

    // ─── Event loop ───────────────────────────────────────────────────────────
    loop {
        // Check IPC signals
        if state.stop_rx.try_recv().is_ok() {
            break;
        }
        if state.show_rx.try_recv().is_ok() {
            state.show(&qh);
            if let Err(e) = event_queue.roundtrip(&mut state) {
                eprintln!("roundtrip after show error: {:?}", e);
                break;
            }
        }

        if state.visible {
            // Non-blocking dispatch of pending events
            if let Err(e) = event_queue.dispatch_pending(&mut state) {
                eprintln!("dispatch_pending error: {:?}", e);
                break;
            }
            if let Err(e) = conn.flush() {
                eprintln!("conn flush error: {:?}", e);
                break;
            }

            // Check whether the UI produced a selection (via draw_ui side-effect)
            // We do a lightweight "dry run" pass just to detect clicks.
            // The actual paint happens below.
            let selected_item = {
                let history_arc = state.history.clone();
                let texture_cache = &mut state.texture_cache as *mut _;
                let popup_origin = state.popup_origin;
                let egui_ctx = state.egui_ctx.clone();
                // Use a separate raw_input with no events so we don't consume real input
                let probe_input = egui::RawInput::default();
                let tc = unsafe { &mut *texture_cache };
                let selected = std::cell::Cell::new(None);
                let _ = egui_ctx.run(probe_input, |ctx| {
                    let mut history = history_arc.lock().unwrap();
                    let item = draw_ui(ctx, &mut *history, popup_origin, tc);
                    selected.set(item);
                });
                selected.into_inner()
            };

            if let Some(_item) = selected_item {
                state.hide();
                InputUtils::paste_content();
            } else {
                state.render_frame();
            }

            std::thread::sleep(std::time::Duration::from_millis(16));
        } else {
            // Keep polling so IPC show/stop signals can reopen the window while hidden.
            if let Err(e) = event_queue.dispatch_pending(&mut state) {
                eprintln!("blocking_dispatch error: {:?}", e);
                break;
            }
            if let Err(e) = conn.flush() {
                eprintln!("conn flush error: {:?}", e);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    eprintln!("Clipboard manager exiting.");
}
