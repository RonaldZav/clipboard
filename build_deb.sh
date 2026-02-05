#!/bin/bash

APP_NAME="clipboard"
VERSION="0.1.6"
ARCH="amd64"
DEB_NAME="${APP_NAME}_${VERSION}_${ARCH}"
BUILD_DIR="deb_build/${DEB_NAME}"

echo "🚧 Iniciando construcción del paquete .deb para $APP_NAME..."

echo "📦 Compilando binario release..."
cargo build --release
if [ $? -ne 0 ]; then
    echo "❌ Error en la compilación."
    exit 1
fi

echo "📂 Creando estructura de directorios..."
rm -rf deb_build
mkdir -p "$BUILD_DIR/DEBIAN"
mkdir -p "$BUILD_DIR/usr/bin"
mkdir -p "$BUILD_DIR/usr/lib/systemd/user"
mkdir -p "$BUILD_DIR/usr/share/applications"
mkdir -p "$BUILD_DIR/usr/share/metainfo" # Directorio para AppStream

echo "COPY: Binario..."
cp "target/release/$APP_NAME" "$BUILD_DIR/usr/bin/$APP_NAME"
chmod 755 "$BUILD_DIR/usr/bin/$APP_NAME"

echo "CREATE: Systemd Service..."
cat <<EOF > "$BUILD_DIR/usr/lib/systemd/user/$APP_NAME.service"
[Unit]
Description=Clipboard Manager Service (Windows 11 Style)
After=graphical-session.target

[Service]
ExecStart=/usr/bin/$APP_NAME --start
Restart=always
RestartSec=3

[Install]
WantedBy=default.target
EOF

echo "CREATE: Desktop Entry..."
cat <<EOF > "$BUILD_DIR/usr/share/applications/$APP_NAME.desktop"
[Desktop Entry]
Name=Clipboard Manager
Comment=Historial de portapapeles Open Source estilo Windows 11
Exec=/usr/bin/$APP_NAME
Icon=utilities-terminal
Type=Application
Categories=Utility;
Terminal=false
StartupNotify=false
EOF

echo "CREATE: AppStream Metainfo..."
cat <<EOF > "$BUILD_DIR/usr/share/metainfo/io.github.ronaldzav.clipboard.metainfo.xml"
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>io.github.ronaldzav.clipboard</id>
  <metadata_license>CC0-1.0</metadata_license>
  <project_license>MIT</project_license>
  <name>Clipboard Manager</name>
  <summary>Historial de portapapeles Open Source estilo Windows 11</summary>
  <description>
    <p>
      Un gestor de portapapeles Open Source, ligero y rápido escrito en Rust.
      Diseñado para imitar la funcionalidad y estética del historial de portapapeles de Windows 11 (Win+V).
    </p>
    <p>
      Este proyecto es de código abierto y puedes encontrar el código fuente en GitHub.
    </p>
    <ul>
      <li>Historial de texto e imágenes.</li>
      <li>Interfaz moderna y minimalista.</li>
      <li>Funciona en segundo plano con bajo consumo de recursos.</li>
      <li>Soporte para atajos de teclado globales.</li>
    </ul>
  </description>
  <launchable type="desktop-id">$APP_NAME.desktop</launchable>
  <url type="homepage">https://ronaldzav.com</url>
  <url type="bugtracker">https://github.com/ronaldzav/clipboard/issues</url>
  <url type="vcs-browser">https://github.com/ronaldzav/clipboard</url>
  <developer_name>Ronald</developer_name>
  <update_contact>ronaldzav@ronaldzav.com</update_contact>
</component>
EOF

echo "CREATE: Script de configuración de atajo..."
SHORTCUT_SCRIPT="$BUILD_DIR/usr/bin/$APP_NAME-setup-shortcut"
cat <<EOF > "$SHORTCUT_SCRIPT"
#!/bin/bash
# Script para configurar Super+V en GNOME

echo "Configurando atajo Super+V para Clipboard Manager..."

if [ "\$XDG_CURRENT_DESKTOP" != "GNOME" ] && [ "\$XDG_CURRENT_DESKTOP" != "ubuntu:GNOME" ]; then
    echo "Aviso: Este script está diseñado para GNOME. Si usas otro entorno, configura el atajo manualmente."
fi

KEY_PATH="/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom-clipboard/"
NAME="Clipboard Manager"
CMD="/usr/bin/$APP_NAME"
BINDING="<Super>v"
SCHEMA="org.gnome.settings-daemon.plugins.media-keys"
KEY="custom-keybindings"

if ! command -v gsettings &> /dev/null; then
    echo "Error: gsettings no encontrado."
    exit 1
fi

CURRENT_LIST=\$(gsettings get \$SCHEMA \$KEY)

if [[ "\$CURRENT_LIST" == *"\$KEY_PATH"* ]]; then
    echo "El atajo ya está en la lista."
else
    echo "Añadiendo entrada a gsettings..."
    if [[ "\$CURRENT_LIST" == "@as []" ]] || [[ "\$CURRENT_LIST" == "[]" ]]; then
        NEW_LIST="['\$KEY_PATH']"
    else
        LEN=\${#CURRENT_LIST}
        SUB=\${CURRENT_LIST:0:LEN-1}
        NEW_LIST="\${SUB}, '\$KEY_PATH']"
    fi
    gsettings set \$SCHEMA \$KEY "\$NEW_LIST"
fi

gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:\$KEY_PATH name "\$NAME"
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:\$KEY_PATH command "\$CMD"
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:\$KEY_PATH binding "\$BINDING"

echo "✅ Atajo configurado correctamente (Super+V)."
EOF
chmod 755 "$SHORTCUT_SCRIPT"

echo "CREATE: Control file..."
cat <<EOF > "$BUILD_DIR/DEBIAN/control"
Package: $APP_NAME
Version: $VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Depends: libxcb-render0, libxcb-shape0, libxcb-xfixes0, libxkbcommon0, libssl3, libgtk-3-0
Maintainer: Ronald <ronaldzav@ronaldzav.com>
Homepage: https://ronaldzav.com
Description: Open Source Windows 11 style clipboard manager
 Un gestor de portapapeles Open Source escrito en Rust que imita el comportamiento de Windows 11.
 Incluye soporte para texto e imágenes y funciona en segundo plano.
 .
 Código fuente disponible en: https://github.com/ronaldzav/clipboard
EOF

echo "CREATE: Post-install script..."
cat <<EOF > "$BUILD_DIR/DEBIAN/postinst"
#!/bin/bash
if command -v systemctl &> /dev/null; then
    systemctl --global enable $APP_NAME.service
    if [ -n "\$SUDO_USER" ]; then
        systemctl --user -M \$SUDO_USER@.host start $APP_NAME.service 2>/dev/null || true
    fi
fi

echo "-------------------------------------------------------"
echo "✅ Clipboard Manager instalado correctamente."
echo ""
echo "⚠️  PASO FINAL REQUERIDO:"
echo "Para configurar el atajo de teclado (Super+V), ejecuta:"
echo "   $APP_NAME-setup-shortcut"
echo "-------------------------------------------------------"
EOF
chmod 755 "$BUILD_DIR/DEBIAN/postinst"

echo "CREATE: Pre-remove script..."
cat <<EOF > "$BUILD_DIR/DEBIAN/prerm"
#!/bin/bash
if command -v systemctl &> /dev/null; then
    systemctl --global disable $APP_NAME.service
    if [ -n "\$SUDO_USER" ]; then
         systemctl --user -M \$SUDO_USER@.host stop $APP_NAME.service 2>/dev/null || true
    fi
fi
EOF
chmod 755 "$BUILD_DIR/DEBIAN/prerm"

echo "🔨 Construyendo paquete .deb..."
dpkg-deb --build "$BUILD_DIR"

echo "🎉 ¡Éxito! Paquete creado en: deb_build/${DEB_NAME}.deb"
