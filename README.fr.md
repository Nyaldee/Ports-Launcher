# Ports Launcher

<p align="center">
  <img src="Ports Launcher.jpg" alt="Ports Launcher screenshot">
</p>

*[Read in English](README.md)*

Une bibliothèque/installateur léger pour les builds "recomp" et source-ports natifs de jeux console, sous Windows : parcours un catalogue, installe directement depuis une release GitHub/GitLab (ou un lien de téléchargement direct), lance, et garde tout à jour depuis une seule fenêtre. Écrit en Rust et [Slint](https://slint.dev), entièrement navigable au clavier *et* à la manette, rien à installer séparément (un 7-Zip minimal est fourni à côté de l'exécutable pour extraire les releases des ports).

**Ports Launcher lui-même ne distribue aucun fichier de jeu, ROM, ISO, ni aucun autre asset protégé par le droit d'auteur.** Chaque entrée du catalogue n'installe jamais que le *code* du recomp/source-port — un build open-source publié par le dépôt du projet lui-même. Toute entrée qui a besoin des données originales du jeu le précise explicitement dans ses instructions **Required files**, et attend que tu les fournisses toi-même depuis une copie du jeu que tu possèdes déjà légalement ; Ports Launcher n'embarque, ne télécharge, n'héberge et ne renvoie jamais vers ces données, nulle part. Utiliser ta propre copie originale, acquise légalement, ne dépend que de toi — Ports Launcher n'a aucun moyen de vérifier d'où vient ce fichier, et n'en assume aucune responsabilité.

## Fonctionnalités

- Bibliothèque pilotée par catalogue (`ports.json`) — ajoute un nouveau port en éditant un simple fichier JSON, aucun code à toucher
- **Ton propre catalogue local** (`ports.local.json`) — ajoute des ports que tu gères toi-même (sans source à télécharger, tu places les fichiers du jeu à la main sous `Library/`) dans un fichier séparé, jamais touché par une mise à jour de `ports.json` — tes ajouts survivent toujours
- Installation en un clic : télécharge automatiquement le bon asset de release pour ton architecture (releases GitHub ou GitLab, ou URL directe), l'extrait, et déplie un dossier-wrapper racine unique si l'archive en contient un
- Détection de mise à jour par port — un port installé reçoit un bouton **Update** à côté de **Play** dès qu'une release plus récente sort, basé à la fois sur le tag de la release *et* sur sa date de publication/asset (donc un projet qui recycle toujours le même tag "latest" est quand même détecté)
- Ports Launcher vérifie aussi ses propres releases GitHub : le bouton **GitHub** du pied de page devient **Update** quand une nouvelle version du launcher lui-même est sortie. Le clic ouvre toujours simplement la page GitHub — rien n'est téléchargé ni remplacé automatiquement, c'est à toi de récupérer et d'installer le nouveau build
- Désinstallation en un clic, en préservant ta sauvegarde si elle vit dans le dossier même du port — une sauvegarde qui vit ailleurs (ex: sous `%APPDATA%`) n'est de toute façon jamais touchée ; le visuel de la boîte est téléchargé une fois puis mis en cache localement
- Panneau Info par port : version/tag installé, instructions d'installation (texte sélectionnable/copiable), et liens en un clic vers le site, la page de mods, le dossier d'installation et le(s) dossier(s) de sauvegarde
- Bouton **Change version** dans le panneau Info — choisis parmi les dernières releases GitHub/GitLab et installe-en une autre que la dernière, utile quand la version la plus récente laisse tomber une plateforme dont tu as besoin (ex: un build Windows)
- Mode "Bibliothèque" plein écran (`Alt+Entrée`) — tous les ports *installés* sous forme de grille de cartes, façon Steam Big Picture
- Navigation manette complète (XInput) *et* navigation clavier complète (flèches, Entrée, Échap) partout dans l'appli, y compris dans chaque dialogue — parcourir, installer, lancer, ouvrir les infos, choisir un fichier/exécutable, et sortir, à la manette comme au clavier
- Chaque dialogue reprend la barre de titre, la police et le dimensionnement de la fenêtre principale, et s'agrandit pour son propre contenu — un message ou un nom de fichier long n'est jamais coupé
- Plus de 100 thèmes de couleur prêts à l'emploi, changeables en direct depuis le sélecteur intégré à Settings avec aperçu instantané (même format `themes.json` que [MAGI Launcher](https://github.com/Nyaldee/MAGI-Launcher))
- Instance unique — relancer l'appli refocalise simplement la fenêtre déjà ouverte

## Raccourcis clavier

| Touche | Action |
|---|---|
| Taper | Filtre le catalogue en direct (recherche floue) |
| `↑` / `↓` ou `Ctrl+W` / `Ctrl+S` | Déplace la sélection haut / bas |
| `←` / `→` ou `Ctrl+A` / `Ctrl+D` | Saute d'une page (10 lignes, liste fenêtrée) / d'une colonne (grille Bibliothèque) |
| `Entrée` | Installe le port sélectionné s'il n'est pas encore installé, le lance sinon |
| `Shift+Entrée` | Ouvre le dossier d'installation du port sélectionné dans l'Explorateur |
| `Alt+Entrée` | Bascule le mode Bibliothèque plein écran |
| `Ctrl+1`...`Ctrl+9` / `Ctrl+0` | Redimensionne le lanceur fenêtré à 10%...90% / 100% de la taille de l'écran, mode fenêtré uniquement |
| `Ctrl+-` / `Ctrl+=` | Réduit / agrandit la bordure de 1px, mode fenêtré uniquement |
| `Échap` | Ferme le dialogue au premier plan s'il y en a un, sort du mode Bibliothèque s'il est actif, sinon ferme le lanceur |

La barre de recherche garde toujours le focus clavier dans la fenêtre principale — chaque touche ci-dessus y est interceptée directement. Un dialogue ouvert par-dessus (Info, Settings, progression d'install, sélecteur de fichier/exécutable...) a son propre focus clavier : les flèches — ou les mêmes alias `Ctrl+W`/`A`/`S`/`D` — y déplacent la sélection, `Entrée` l'active, `Échap` le ferme (sauf le dialogue de progression d'install, qui ne peut pas être interrompu).

## Manette

Branche une manette XInput (façon Xbox) et chaque fenêtre y répond immédiatement, sans configuration — le même routeur d'entrées fonctionne sur la bibliothèque principale et sur chaque dialogue ouvert par-dessus. Déplacer la sélection à la souris et à la manette/au clavier reste toujours synchronisé — une seule chose est jamais mise en surbrillance à la fois, peu importe comment tu l'y as amenée :

| Bouton | Action |
|---|---|
| Croix directionnelle / stick gauche | Déplace la sélection |
| `A` ou `Start` | Installe / Lance le port sélectionné (comme `Entrée`) |
| `B` | Sort du dialogue courant |
| `X` | Ouvre le panneau Info du port sélectionné |
| `Back` | Bascule le mode Bibliothèque plein écran |

## Mode Bibliothèque

`Alt+Entrée` bascule vers une grille plein écran de tous les ports actuellement *installés* — comme la bibliothèque Big Picture de Steam. Les ports pas encore installés n'apparaissent que dans la vue liste fenêtrée, jamais en mode Bibliothèque. `Échap` (ou `Alt+Entrée` à nouveau) revient à la vue fenêtrée.

## Panneau Info

Sélectionne un port et ouvre son panneau **Info** (bouton, ou `X` à la manette) pour sa version/tag installée, les instructions d'installation éventuelles de `ports.json` (texte sélectionnable, copiable/collable), et des liens en un clic vers son site, sa page de mods, son dossier d'installation et son dossier de sauvegarde — plus un bouton **Save folder 2** pour les ports avec un second emplacement de sauvegarde (`save_folder2`, voir plus bas). N'importe lequel de ces boutons est simplement désactivé s'il n'existe pas encore (pas installé, le jeu n'a pas encore créé de sauvegarde, ou le port n'a tout simplement pas de second emplacement). `↑`/`↓` (ou la croix directionnelle/le stick de la manette) fait défiler le texte d'instructions quand il est trop long pour tenir ; `←`/`→` déplace la sélection entre les boutons, `Entrée`/`A` active celui qui est en surbrillance.

À côté du texte de version, un bouton **Change version** (ports GitHub/GitLab uniquement) récupère les dernières releases disponibles et permet d'en installer une autre que la dernière — pratique quand la version la plus récente n'a pas de build pour ta plateforme, ou pour simplement revenir en arrière.

## Réglages

Ouvre-le depuis le bouton **◯** de la barre de titre — un sélecteur cherchable de façon floue et en direct parmi tous les thèmes de `themes.json`. Déplacer la sélection (survol souris, ou `↑`/`↓`/le stick de la manette) prévisualise un thème instantanément sur toute l'appli ; `Entrée`/clic dessus valide le choix, en le réécrivant directement dans `themes.json`. Fermer le dialogue sans valider (`Échap`) revient au thème réellement actif avant l'ouverture — seule une vraie sélection est écrite sur le disque. Une rangée de boutons de raccourci sous la liste — **Library**, **ports.json**, **ports.local.json**, **state.json**, **themes.json** — ouvre directement ce dossier/fichier dans l'Explorateur, grisé s'il n'existe pas encore ; `←`/`→` (ou la manette) déplace la sélection entre eux, `↑`/`↓` rend le focus à la liste de thèmes.

## Mises à jour

Deux choses indépendantes peuvent être "obsolètes," chacune gérée différemment :

- **Un jeu/port que tu as installé** — vérifié en tâche de fond (limité à une fois toutes les 12 heures, pour rester sous le quota d'API non authentifiée de GitHub/GitLab) contre la dernière release de son dépôt. Un tag plus récent *ou* une date de publication/asset plus récente le signale — la vérification par date existe spécifiquement pour les projets qui ne bump jamais leur tag et se contentent de remplacer le fichier de la même release "latest". Apparaît comme un bouton **Update** à côté de **Play**.
- **Ports Launcher lui-même** — même vérification tag/date, faite contre les propres releases de ce dépôt. Quand une nouvelle est dispo, le bouton **GitHub** du pied de page devient **Update** ; le lien ne change pas, donc c'est toujours à toi de récupérer et d'installer le nouveau build.

## Outils utiles

Deux outils externes reviennent régulièrement dans les instructions **Required files** de `ports.json`, pour préparer tes propres données de jeu avant de les pointer vers un port :

- **[7-Zip](https://github.com/ip7z/7zip)** — une version minimale (ligne de commande) est fournie avec Ports Launcher, mais uniquement pour extraire en interne les releases des ports ; récupère la version classique/graphique séparément pour décompresser un dump `.7z` (ou autre format compressé) d'un jeu que tu possèdes déjà.
- **[extract-xiso](https://github.com/XboxDev/extract-xiso)** — pas fourni avec Ports Launcher, à récupérer séparément : extrait les fichiers individuels d'un `.iso` Xbox/Xbox 360 ; plusieurs ports Xbox 360 demandent explicitement `extract-xiso.exe` dans leurs instructions pour obtenir leur dossier `assets`.

## Configuration

### `ports.json`

Le catalogue principal, à côté de l'exécutable — jamais embarqué dans le `.exe`, donc lui (et le lanceur lui-même) peuvent être mis à jour indépendamment de n'importe quel port. Chaque entrée décrit un port installable :

```json
{
  "ports": [
    {
      "name": "Ape Escape (ApeEscapeRecomp)",
      "tags": ["Saru", "PS1", "Platformer", "Singleplayer"],
      "source": "https://github.com/mstan/ApeEscapeRecomp",
      "folder_name": "ApeEscapeRecomp",
      "instructions": "Required files: Ape Escape (USA).bin (CRC32 : C6F455BC) + PSX - SCPH1001.BIN\n\nLanguages: English.\nNothing to do, the port guides you.",
      "image_url": "https://cdn2.steamgriddb.com/grid/dce0cff3ad30897876b169eb066662dd.png",
      "save_folder": "saves"
    }
  ]
}
```

- **`name`** (obligatoire) — nom affiché, cherchable de façon floue.
- **`tags`** — labels libres affichés à côté du nom et inclus dans la recherche floue.
- **`source`** (optionnel) — une URL de dépôt GitHub ou GitLab (installe depuis la dernière release de ce dépôt, en choisissant automatiquement le bon asset), ou une URL de téléchargement direct. Peut être un dict par plateforme au lieu d'une simple chaîne (ex: `{"windows": "...", "linux": "..."}`) — le format laisse la porte ouverte à un futur build non-Windows, mais ce build-ci ne résout jamais que la valeur `"windows"`. Omets `source` entièrement pour un port que tu installes/gères toi-même — voir [`ports.local.json`](#portslocaljson) plus bas.
- **`folder_name`** (obligatoire) — sous-dossier de `Library/` dans lequel le port est installé.
- **`executable`** (optionnel) — chemin vers l'exécutable, relatif à `folder_name` ; peut être un dict par plateforme comme `source`. Omets-le pour auto-détecter le seul `.exe` ou `.lnk` présent — un choix manuel n'est demandé que si plusieurs candidats sont trouvés. Un raccourci `.lnk` est lancé via le Shell Windows plutôt qu'exécuté directement, ce qui permet justement de passer des arguments en ligne de commande à l'exécutable cible : fais pointer la cible du raccourci vers `jeu.exe --un-argument` et dépose le `.lnk` dans le dossier du port.
- **`website`** (optionnel) — lien vers la page d'accueil affiché dans le panneau Info. Par défaut, reprend `source` lui-même quand c'est déjà une URL GitHub/GitLab.
- **`instructions`** (optionnel) — texte libre affiché dans le panneau Info (fichiers requis, étapes d'installation, notes de langue...).
- **`mods_url`** (optionnel) — lien vers la page de mods pour le panneau Info.
- **`image_url`** (optionnel) — visuel de la boîte ; téléchargé une fois puis mis en cache localement.
- **`save_folder`** (optionnel) — chemin vers les données de sauvegarde : absolu (avec des variables d'environnement comme `%LOCALAPPDATA%` étendues), ou relatif au dossier d'installation. Affiché comme un lien en un clic dans le panneau Info. Si elle vit *dans* le dossier d'installation, elle est automatiquement préservée lors d'une désinstallation/réinstallation plutôt que supprimée avec le reste.
- **`save_folder2`** (optionnel) — un second emplacement de sauvegarde indépendant, pour un port qui répartit ses données sur deux endroits (ex : les réglages sous `%APPDATA%` plus une sauvegarde dans le dossier d'installation, ou un mode "sauvegarde portable" en plus d'un mode normal). Mêmes règles que `save_folder` en tout point — son propre bouton **Save folder 2** dans le panneau Info, préservé lors d'une désinstallation/réinstallation de la même façon s'il vit dans le dossier d'installation — juste suivi et sauvegardé complètement à part, pour que les deux ne puissent jamais se percuter ou s'écraser l'un l'autre, même s'ils contiennent des fichiers au nom identique.

Concrètement, cette préservation "lors d'une désinstallation/réinstallation" n'a rien de magique : désinstaller un port dont la sauvegarde vit dans le dossier d'installation la déplace vers `Library/.saves_backup/<folder_name>/save_folder/` (ou `.../save_folder2/` pour le second champ) juste avant que le reste du dossier ne soit supprimé, puis une installation ultérieure de ce même port la remet directement en place et supprime la sauvegarde temporaire. C'est un dossier caché (préfixé d'un point), jamais touché par autre chose que Ports Launcher lui-même — utile à savoir si tu fouilles `Library/` à la main pour une sauvegarde qui semble avoir disparu en pleine réinstallation, ou si une installation est interrompue et qu'il faut la récupérer manuellement.

### `ports.local.json`

Ton propre catalogue, à côté de `ports.json` — pour les ports que tu gères toi-même plutôt que d'installer via Ports Launcher : tu crées le dossier sous `Library/`, tu y places les fichiers du jeu à la main, et il apparaît exactement comme n'importe quel autre port (jouable, a un panneau Info, apparaît en mode Bibliothèque une fois "installé"). Même format d'entrée que `ports.json`, sauf que `source` ne s'applique jamais ici — omets-le, ou laisse-le vide. Ce fichier vivant à part, remplacer `ports.json` par une version plus récente du mainteneur ne touche jamais à ce que tu as ajouté ici.

Un `folder_name` qui entre en collision avec celui d'une entrée de `ports.json` remplace l'entrée officielle par la tienne. Le bouton **Uninstall** se comporte aussi différemment pour un port local : il ne supprime jamais rien — il ouvre le dossier du port dans l'Explorateur à la place, pour que tu gardes le contrôle total de fichiers que Ports Launcher n'a jamais téléchargés lui-même.

Le dépôt fournit un `ports.local.json` avec deux exemples désactivés (`"name"`/`"folder_name"` mis à `null`, ce qui fait que Ports Launcher les ignore) — copie-en un, remplis de vraies valeurs, et ça devient une vraie entrée.

### `themes.json`

```json
{
  "theme": "arc-dark",
  "font_family": "Segoe UI",
  "placeholder_text": "Type to search...",
  "show_clock": true,
  "window_size": 60,
  "border": 1,
  "themes": {
    "arc-dark": {
      "search_background": "#404552",
      "search_text": "#7c818c",
      "list_background": "#383c4a",
      "list_text": "#d3dae3",
      "selected_background": "#5294e2",
      "selected_text": "#ffffff",
      "border": "#4b5162"
    }
  }
}
```

Même format que `themes.json` de [MAGI Launcher](https://github.com/Nyaldee/MAGI-Launcher) — change de thème en direct depuis le sélecteur intégré (voir [Réglages](#réglages) plus haut), éditer la clé `theme` à la main ne sert qu'à définir celui du tout premier lancement. Chaque dialogue (Info, progression d'install, sélecteurs de fichier, boîtes d'erreur/message) suit le même thème aussi, y compris les couleurs de sélection de texte. `show_clock` affiche ou masque une horloge à côté de la barre de recherche (le format d'heure court de Windows, celui réglé dans Paramètres > Heure et langue) — vaut `true` par défaut, mets `false` pour la masquer. `window_size` (0-100, un pourcentage de la taille d'écran) et `border` (en pixels) sont les valeurs de départ du mode fenêtré — les deux se mettent aussi à jour en direct et se persistent dans ce fichier via `Ctrl+1`...`Ctrl+0`/`Ctrl+-`/`Ctrl+=` (voir [Raccourcis clavier](#raccourcis-clavier)), les éditer à la main ne sert donc qu'à changer la taille du tout premier lancement.

### `state.json`

Fichier interne (versions installées, état de la fenêtre) que Ports Launcher écrit tout seul — tu ne devrais pas avoir besoin d'y toucher. C'est aussi le seul moyen d'augmenter le quota d'API non authentifiée GitHub/GitLab pour les vérifications de mise à jour : ajoute une clé `"github_token"`/`"gitlab_token"` à la main (pas de champ dédié dans l'UI pour l'instant). Ce token est alors stocké en **texte clair**. Ne partage jamais ce fichier, ne l'upload nulle part, et ne le laisse jamais visible sur un stream/partage d'écran — contrairement à `ports.json`/`themes.json`/`ports.local.json`, il peut contenir un identifiant.

## Crédits

Construit avec [Claude](https://claude.com) (l'assistant de code IA d'Anthropic).

## Licence

Copyright (C) 2026 Nyaldee. Distribué sous licence [GNU General Public License v3.0](LICENSE) — voir le fichier `LICENSE` pour le texte complet.
