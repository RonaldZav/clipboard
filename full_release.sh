#!/bin/bash

set -e
echo "Iniciando proceso de compilacion y empaquetado..."

chmod +x build_deb.sh

./build_deb.sh

echo ""
echo "Ejecuta: sudo dpkg -i deb_build/clipboard_*.deb"
