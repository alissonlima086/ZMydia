#!/usr/bin/env sh
set -eu

VERSION="__ZMYDIA_VERSION__"
ARCHIVE_SHA256="__ZMYDIA_ARCHIVE_SHA256__"
RELEASE_BASE_URL="${ZMYDIA_RELEASE_BASE_URL:-https://github.com/alissonlima086/ZMydia/releases/download/v${VERSION}}"
ARCHIVE_NAME="zmydia-${VERSION}-linux-x86_64.tar.gz"
APP_URL="http://127.0.0.1:4173"
WORK_DIR=""

cleanup() {
  if [ -n "$WORK_DIR" ] && [ -d "$WORK_DIR" ]; then
    rm -rf "$WORK_DIR"
  fi
}
trap cleanup 0 HUP INT TERM

say() {
  printf '%s\n' "$*"
}

fail() {
  printf 'Erro: %s\n' "$*" >&2
  exit 1
}

require_release_metadata() {
  case "$VERSION:$ARCHIVE_SHA256" in
    *'__ZMYDIA_'*) fail 'este template ainda não foi preparado para uma release' ;;
  esac
  case "$ARCHIVE_SHA256" in
    *[!0-9a-fA-F]*|'') fail 'SHA-256 inválido no instalador' ;;
  esac
  [ "${#ARCHIVE_SHA256}" -eq 64 ] || fail 'SHA-256 inválido no instalador'
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "comando obrigatório não encontrado: $1"
}

installed_server() {
  printf '%s/.local/bin/zmydia-server\n' "$HOME"
}

download_archive() {
  destination="$1"
  url="${RELEASE_BASE_URL}/${ARCHIVE_NAME}"
  if command -v curl >/dev/null 2>&1; then
    curl --fail --location --show-error --progress-bar "$url" --output "$destination"
  elif command -v wget >/dev/null 2>&1; then
    wget --output-document="$destination" "$url"
  else
    fail 'instale curl ou wget para baixar o ZMydia'
  fi
}

write_launcher() {
  launcher="$HOME/.local/bin/zmydia"
  desktop_dir="$HOME/.local/share/applications"
  desktop="$desktop_dir/zmydia.desktop"
  mkdir -p "$HOME/.local/bin" "$desktop_dir"
  cat >"$launcher" <<'EOF'
#!/usr/bin/env sh
set -eu
systemctl --user start zmydia.service
url='http://127.0.0.1:4173'
if command -v xdg-open >/dev/null 2>&1; then
  exec xdg-open "$url"
fi
printf 'Abra %s no navegador.\n' "$url"
EOF
  chmod 755 "$launcher"
  cat >"$desktop" <<EOF
[Desktop Entry]
Type=Application
Name=ZMydia
Comment=Biblioteca pessoal de vídeos
Exec=$launcher
Icon=zmydia
Terminal=false
Categories=AudioVideo;Video;
StartupNotify=false
EOF
  chmod 644 "$desktop"
}

install_or_update() {
  require_release_metadata
  for command_name in find mktemp sha256sum systemctl tar; do
    require_command "$command_name"
  done
  [ -n "${HOME:-}" ] && [ "$HOME" != "/" ] || fail 'HOME inválido'

  WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/zmydia-installer.XXXXXX")"
  archive="$WORK_DIR/$ARCHIVE_NAME"
  say "Baixando ZMydia ${VERSION}..."
  download_archive "$archive"
  printf '%s  %s\n' "$ARCHIVE_SHA256" "$archive" | sha256sum --check --status \
    || fail 'o arquivo baixado não corresponde ao SHA-256 da release'

  tar -xzf "$archive" -C "$WORK_DIR"
  package_dir="$(find "$WORK_DIR" -mindepth 1 -maxdepth 1 -type d -name 'zmydia-*-linux-x86_64' -print -quit)"
  [ -n "$package_dir" ] && [ -x "$package_dir/bin/zmydia-server" ] \
    || fail 'o pacote baixado é inválido'

  "$package_dir/bin/zmydia-server" service install
  write_launcher
  cleanup
  WORK_DIR=""
  say ""
  say "Instalação concluída. Abra $APP_URL"
}

require_installed() {
  server="$(installed_server)"
  [ -x "$server" ] || fail 'ZMydia não está instalado para este usuário'
}

service_action() {
  action="$1"
  require_installed
  "$(installed_server)" service "$action"
}

open_app() {
  require_installed
  systemctl --user start zmydia.service
  if command -v xdg-open >/dev/null 2>&1; then
    xdg-open "$APP_URL" >/dev/null 2>&1 || true
  else
    say "Abra $APP_URL no navegador."
  fi
}

uninstall_app() {
  require_installed
  "$(installed_server)" service uninstall
  rm -f "$HOME/.local/bin/zmydia" "$HOME/.local/share/applications/zmydia.desktop"
  say 'Os dados e a configuração foram preservados.'
}

purge_app() {
  if [ -x "$(installed_server)" ]; then
    uninstall_app
  fi
  [ -n "${HOME:-}" ] && [ "$HOME" != "/" ] || fail 'HOME inválido'
  rm -rf \
    "$HOME/.local/share/zmydia" \
    "$HOME/.cache/zmydia" \
    "$HOME/.config/zmydia"
  say 'Aplicativo, biblioteca, cache e configuração foram removidos.'
}

confirm_purge() {
  say 'ATENÇÃO: esta ação apagará permanentemente a biblioteca e a configuração.'
  printf 'Digite APAGAR para continuar: '
  IFS= read -r confirmation
  [ "$confirmation" = 'APAGAR' ] || {
    say 'Operação cancelada.'
    return 0
  }
  purge_app
}

pause_menu() {
  printf '\nPressione Enter para continuar...'
  IFS= read -r _unused
}

show_menu() {
  while :; do
    say ''
    say "ZMydia ${VERSION}"
    say '1. Instalar ou atualizar'
    say '2. Abrir no navegador'
    say '3. Ver status'
    say '4. Reiniciar serviço'
    say '5. Acompanhar logs'
    say '6. Desinstalar preservando os dados'
    say '7. Remover completamente'
    say '0. Sair'
    printf 'Escolha uma opção: '
    IFS= read -r choice
    case "$choice" in
      1) install_or_update; pause_menu ;;
      2) open_app; pause_menu ;;
      3) service_action status || true; pause_menu ;;
      4) service_action restart; pause_menu ;;
      5) service_action logs || true ;;
      6) uninstall_app; pause_menu ;;
      7) confirm_purge; pause_menu ;;
      0) exit 0 ;;
      *) say 'Opção inválida.' ;;
    esac
  done
}

relaunch_in_terminal() {
  [ "${ZMYDIA_INSTALLER_TERMINAL:-0}" != '1' ] || return 1
  [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] || return 1
  script_path="$0"
  if command -v xdg-terminal-exec >/dev/null 2>&1; then
    ZMYDIA_INSTALLER_TERMINAL=1 xdg-terminal-exec sh "$script_path" --menu &
  elif command -v kitty >/dev/null 2>&1; then
    ZMYDIA_INSTALLER_TERMINAL=1 kitty --hold sh "$script_path" --menu &
  elif command -v konsole >/dev/null 2>&1; then
    ZMYDIA_INSTALLER_TERMINAL=1 konsole --hold -e sh "$script_path" --menu &
  elif command -v gnome-terminal >/dev/null 2>&1; then
    ZMYDIA_INSTALLER_TERMINAL=1 gnome-terminal --wait -- sh "$script_path" --menu &
  elif command -v x-terminal-emulator >/dev/null 2>&1; then
    ZMYDIA_INSTALLER_TERMINAL=1 x-terminal-emulator -e sh "$script_path" --menu &
  else
    return 1
  fi
  return 0
}

print_help() {
  cat <<EOF
ZMydia Linux installer ${VERSION}

USO:
  sh $(basename "$0")                 Menu interativo
  sh $(basename "$0") --install       Instala ou atualiza
  sh $(basename "$0") --uninstall     Desinstala e preserva os dados
  sh $(basename "$0") --purge         Desinstala e apaga os dados
  sh $(basename "$0") --status        Mostra o status do serviço
  sh $(basename "$0") --logs          Acompanha os logs
  sh $(basename "$0") --restart       Reinicia o serviço
  sh $(basename "$0") --open          Abre a interface no navegador
EOF
}

case "${1:---menu}" in
  --menu)
    if [ ! -t 0 ] || [ ! -t 1 ]; then
      relaunch_in_terminal && exit 0
      fail 'abra um terminal e execute este instalador com sh'
    fi
    show_menu
    ;;
  --install|--update) install_or_update ;;
  --uninstall) uninstall_app ;;
  --purge) confirm_purge ;;
  --status) service_action status ;;
  --logs) service_action logs ;;
  --restart) service_action restart ;;
  --open) open_app ;;
  --help|-h) print_help ;;
  *) print_help >&2; exit 2 ;;
esac
