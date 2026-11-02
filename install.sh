#!/bin/sh
# Installe / met à jour E-cart-é pour l'utilisateur courant (antiX / Debian, IceWM).
#   ./install.sh            installe ou met à jour vers la dernière version
#   ./install.sh --force    réinstalle même si déjà à jour
#   ./install.sh --uninstall
#
# Dépôt privé : fournir l'accès par l'une de ces méthodes
#   - gh (GitHub CLI) connecté : `gh auth login`
#   - variable GITHUB_TOKEN / GH_TOKEN, ou fichier ~/.config/ecarte/token (jeton avec lecture "Contents")
set -eu

REPO="Paulem79/ecarte"
ASSET="ecarte-linux-x86_64.tar.gz"

BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"
ICON_DIR="${HOME}/.local/share/icons/hicolor"
STATE_DIR="${HOME}/.local/share/ecarte"
TAG_FILE="${STATE_DIR}/installed-tag"
TOKEN_FILE="${HOME}/.config/ecarte/token"

say() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merreur :\033[0m %s\n' "$*" >&2; exit 1; }

FORCE=0
case "${1:-}" in
  --force) FORCE=1 ;;
  --uninstall)
    rm -f "${BIN_DIR}/ecarte" "${BIN_DIR}/ecarte-update" "${APP_DIR}/ecarte.desktop" \
          "${ICON_DIR}/scalable/apps/ecarte.svg" "${ICON_DIR}/48x48/apps/ecarte.png"
    rm -rf "${STATE_DIR}"
    if [ -f "${HOME}/.icewm/programs" ]; then
      sed -i '/# ecarte-begin/,/# ecarte-end/d' "${HOME}/.icewm/programs"
    fi
    say "E-cart-é désinstallé (le cache de cartes ~/.local/share/ecarte a été supprimé)."
    exit 0 ;;
  "") ;;
  *) die "option inconnue : $1" ;;
esac

[ "$(uname -m)" = "x86_64" ] || die "architecture $(uname -m) non prise en charge (x86_64 requis)"
command -v tar >/dev/null 2>&1 || die "tar est requis"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM

TOKEN="${GITHUB_TOKEN:-${GH_TOKEN:-}}"
if [ -z "$TOKEN" ] && [ -r "$TOKEN_FILE" ]; then TOKEN="$(tr -d ' \r\n' < "$TOKEN_FILE")"; fi

LATEST=""
fetch_with_gh() {
  command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1 || return 1
  LATEST="$(gh release view --repo "$REPO" --json tagName -q .tagName 2>/dev/null)" || return 1
  [ -n "$LATEST" ] || return 1
  if [ "$FORCE" = 0 ] && [ -f "$TAG_FILE" ] && [ "$(cat "$TAG_FILE")" = "$LATEST" ]; then return 0; fi
  gh release download "$LATEST" --repo "$REPO" --pattern "$ASSET" --dir "$TMP" --clobber >/dev/null
}

fetch_with_curl() {
  command -v curl >/dev/null 2>&1 || die "curl (ou gh) est requis : sudo apt install curl"
  API="https://api.github.com/repos/${REPO}/releases/latest"
  if [ -n "$TOKEN" ]; then
    JSON="$(curl -fsSL -H "Authorization: Bearer $TOKEN" -H "Accept: application/vnd.github+json" "$API")" || return 1
  else
    JSON="$(curl -fsSL -H "Accept: application/vnd.github+json" "$API")" || return 1
  fi
  LATEST="$(printf '%s\n' "$JSON" | sed -n 's/^ *"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)"
  [ -n "$LATEST" ] || return 1
  if [ "$FORCE" = 0 ] && [ -f "$TAG_FILE" ] && [ "$(cat "$TAG_FILE")" = "$LATEST" ]; then return 0; fi
  URL="$(printf '%s\n' "$JSON" | awk -v a="$ASSET" '
    /"url": *"https:\/\/api.github.com\/repos\/[^"]*\/releases\/assets\// { match($0, /https:[^"]*/); u=substr($0, RSTART, RLENGTH) }
    $0 ~ "\"name\": *\"" a "\"" { print u; exit }')"
  [ -n "$URL" ] || die "fichier $ASSET introuvable dans la release $LATEST"
  if [ -n "$TOKEN" ]; then
    curl -fsSL -H "Authorization: Bearer $TOKEN" -H "Accept: application/octet-stream" -o "$TMP/$ASSET" "$URL"
  else
    curl -fsSL -H "Accept: application/octet-stream" -o "$TMP/$ASSET" "$URL"
  fi
}

say "Recherche de la dernière version…"
if ! fetch_with_gh && ! fetch_with_curl; then
  die "impossible de lire les releases de $REPO (réseau coupé, ou dépôt privé sans accès).
Connectez-vous avec :  gh auth login
ou placez un jeton dans ${TOKEN_FILE}  (chmod 600), ou exportez GITHUB_TOKEN."
fi

if [ "$FORCE" = 0 ] && [ -f "$TAG_FILE" ] && [ "$(cat "$TAG_FILE")" = "$LATEST" ] && [ -x "${BIN_DIR}/ecarte" ]; then
  say "Déjà à jour ($LATEST). Utilisez --force pour réinstaller."
  exit 0
fi

[ -f "$TMP/$ASSET" ] || die "téléchargement échoué"
say "Installation de $LATEST…"
tar -C "$TMP" -xzf "$TMP/$ASSET"
SRC="$TMP/ecarte"
[ -f "$SRC/ecarte" ] || die "archive inattendue"

mkdir -p "$BIN_DIR" "$APP_DIR" "$STATE_DIR" "$ICON_DIR/scalable/apps" "$ICON_DIR/48x48/apps"
cp "$SRC/ecarte" "${BIN_DIR}/ecarte.new"
chmod 755 "${BIN_DIR}/ecarte.new"
mv -f "${BIN_DIR}/ecarte.new" "${BIN_DIR}/ecarte"   # remplacement atomique, même si l'appli tourne

# Icônes
[ -f "$SRC/assets/ecarte.svg" ] && cp "$SRC/assets/ecarte.svg" "$ICON_DIR/scalable/apps/ecarte.svg"
[ -f "$SRC/assets/ecarte-48.png" ] && cp "$SRC/assets/ecarte-48.png" "$ICON_DIR/48x48/apps/ecarte.png"

# Script de mise à jour (copie de ce script, lancé via le menu ou `ecarte-update`)
if [ -f "$SRC/install.sh" ]; then
  cp "$SRC/install.sh" "${BIN_DIR}/ecarte-update"
  chmod 755 "${BIN_DIR}/ecarte-update"
fi

# Lanceur .desktop (menu antiX / IceWM)
cat > "${APP_DIR}/ecarte.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=E-cart-é
GenericName=Cartographie
Comment=Créer des cartes thématiques (export SVG, PNG, JPG)
Exec=${BIN_DIR}/ecarte
Icon=ecarte
Terminal=false
Categories=Graphics;Education;Geography;
Keywords=carte;map;cartographie;graticule;
StartupNotify=false
Actions=Update;

[Desktop Action Update]
Name=Mettre à jour E-cart-é
Exec=x-terminal-emulator -e ${BIN_DIR}/ecarte-update
DESK
chmod 644 "${APP_DIR}/ecarte.desktop"

# IceWM : entrée dans le menu « Applications » (fichier ~/.icewm/programs, bloc balisé et idempotent)
ICON_PATH="${ICON_DIR}/48x48/apps/ecarte.png"
[ -f "$ICON_PATH" ] || ICON_PATH="ecarte"
if [ -d "${HOME}/.icewm" ]; then
  PROG="${HOME}/.icewm/programs"
  touch "$PROG"
  sed -i '/# ecarte-begin/,/# ecarte-end/d' "$PROG"
  {
    echo "# ecarte-begin"
    echo "prog \"E-cart-é\" \"${ICON_PATH}\" ${BIN_DIR}/ecarte"
    echo "# ecarte-end"
  } >> "$PROG"
fi
# antiX régénère ses menus depuis les .desktop : on le demande si l'outil existe
for cmd in "desktop-menu --write-out-global" "desktop-menu --write-out"; do
  # shellcheck disable=SC2086
  command -v ${cmd%% *} >/dev/null 2>&1 && { $cmd >/dev/null 2>&1 || true; break; }
done
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APP_DIR" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$ICON_DIR" >/dev/null 2>&1 || true

echo "$LATEST" > "$TAG_FILE"

# PATH : ~/.local/bin
case ":$PATH:" in *":${BIN_DIR}:"*) ;; *) say "Astuce : ajoutez ${BIN_DIR} à votre PATH (déjà fait par défaut sur antiX en nouvelle session)." ;; esac

# Bibliothèques manquantes ?
if command -v ldd >/dev/null 2>&1; then
  MISSING="$(ldd "${BIN_DIR}/ecarte" 2>/dev/null | awk '/not found/ {print $1}' | tr '\n' ' ')"
  if [ -n "$MISSING" ]; then
    printf '\033[1;33mattention :\033[0m bibliothèques manquantes : %s\n  -> sudo apt install libgtk-3-0 libxkbcommon0 libwayland-client0 libgl1\n' "$MISSING"
  fi
fi

say "E-cart-é $LATEST installé."
say "Lancer : menu IceWM > Applications > E-cart-é   (ou commande : ecarte)"
say "Mise à jour : ecarte-update   (ou clic droit sur l'entrée de menu si supporté)"
say "Si l'entrée n'apparaît pas : icewm --restart"
