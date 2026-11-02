# E-cart-é

Outil de cartographie léger (Rust), pensé pour les petites machines : fonds de carte avec
cadrage, graticule et libellés intégrés, annotations (titres, flèches, points, cercles,
rectangles, polygones, textes), coloriage de pays, légende, échelle, flèche du nord,
export **SVG / PNG / JPG**. Linux et Windows natifs.

## Hors ligne d'abord

- Fonctionne **sans connexion** : pays 1:110m, lacs et fleuves 1:110m sont intégrés à l'exécutable.
- Onglet **Données** : télécharge (si besoin) des jeux plus détaillés (pays 1:50m, lacs/fleuves 1:50m,
  régions/États) puis les met en cache ; ils restent utilisables hors ligne.
- Synchronisation optionnelle (au démarrage si en ligne, ou à la demande) : mise à jour via ETag,
  jamais bloquante ; un échec réseau conserve le cache.
- Cache : `%APPDATA%\ecarte` (Windows) ou `~/.local/share/ecarte` (Linux) ; `ECARTE_DATA` pour le changer.

## Fonctions

- Projections : Mercator, équirectangulaire, sinusoïdale, Mollweide, orthographique, azimutale
  équivalente, stéréographique, conique conforme de Lambert.
- Graticule : pas, couleur, pointillés, étiquettes sur les côtés choisis. Cadre simple / double / bandes alternées.
- Éléments : texte (rotation, halo), points (6 formes), cercles en km ou px, lignes et flèches
  (grand cercle optionnel), rectangles, polygones, polylignes ; ordre, visibilité, duplication, annuler/rétablir.
- Fichiers `.ecarte` (JSON). Export PNG/JPG jusqu'à 600 dpi, SVG vectoriel.

Raccourcis : molette = zoom carte ; ✋/clic milieu = déplacer ; S H T P C L A R G K = outils ;
Ctrl+Z/Y, Ctrl+S, Ctrl+O, Suppr.

## Compiler

```sh
# Debian / antiX
sudo apt install build-essential pkg-config libgtk-3-dev libxkbcommon-dev libwayland-dev
cargo build --release
```

Données : [Natural Earth](https://www.naturalearthdata.com/) (domaine public).

## Installation sur antiX / Debian (IceWM)

Le dépôt est privé : le script utilise `gh` (connecté) ou un jeton.

```sh
gh auth login                       # une seule fois (ou: export GITHUB_TOKEN=...)
gh api repos/Paulem79/ecarte/contents/install.sh -H "Accept: application/vnd.github.raw" > install.sh
sh install.sh                       # installe, ou met à jour si déjà installé
```

- Installe dans `~/.local/bin/ecarte`, ajoute le lanceur au menu IceWM (Applications) et l'icône.
- Mise à jour ensuite : commande `ecarte-update` (ou `sh install.sh`). `--force` réinstalle, `--uninstall` supprime.
- Chaque commit sur `main` publie une release (Linux + Windows) via GitHub Actions.
