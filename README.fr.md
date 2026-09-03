# Ports Launcher

<p align="center">
  <img src="Ports Launcher.jpg" alt="Ports Launcher screenshot">
</p>

*[Read in English](README.md)*

> [!IMPORTANT]
> **Ports Launcher lui-même ne distribue aucun fichier de jeu, ROM, ISO, ni aucun autre asset protégé par le droit d'auteur.** Chaque entrée du catalogue n'installe jamais que le *code* du recomp/source-port — un build open-source publié par le dépôt du projet lui-même. Toute entrée qui a besoin des données originales du jeu le précise explicitement dans ses instructions **Required files**, et attend que tu les fournisses toi-même depuis une copie du jeu que tu possèdes déjà légalement ; Ports Launcher n'embarque, ne télécharge, n'héberge et ne renvoie jamais vers ces données, nulle part. Utiliser ta propre copie originale, acquise légalement, ne dépend que de toi — Ports Launcher n'a aucun moyen de vérifier d'où vient ce fichier, et n'en assume aucune responsabilité.

Une bibliothèque/installateur léger pour les builds « recomp » et source-ports natifs de jeux console, sous Windows : parcours un catalogue, installe directement depuis une release GitHub/GitLab (ou un lien de téléchargement direct), lance, et garde tout à jour depuis une seule fenêtre. Écrit en Rust et [Slint](https://slint.dev), entièrement navigable au clavier *et* à la manette, rien à installer séparément (un 7-Zip minimal est fourni à côté de l'exécutable pour extraire les releases des ports).

## Fonctionnalités

- Bibliothèque pilotée par catalogue (`ports.json`) — ajoute un nouveau port en éditant un simple fichier JSON, aucun code à toucher
- **Ton propre catalogue local** (`ports.local.json`) — ajoute des ports que tu gères toi-même (sans source à télécharger, tu places les fichiers du jeu à la main sous `Library/`) dans un fichier séparé, jamais touché par une mise à jour de `ports.json` — tes ajouts survivent toujours
- Installation en un clic : télécharge automatiquement le bon asset de release pour ton architecture (releases GitHub ou GitLab, ou URL directe), l'extrait, et déplie un dossier-wrapper racine unique si l'archive en contient un
- Mise à jour automatique par défaut — appuyer sur **Play** sur un port GitHub/GitLab installé vérifie d'abord silencieusement s'il existe une release plus récente (basé à la fois sur le tag de la release *et* sur sa date de publication/asset, donc un projet qui recycle toujours le même tag « latest » est quand même détecté) et l'installe avant de lancer. Désactivable port par port depuis le bouton **Update** du panneau Info si tu préfères rester figé sur une version précise
- Ports Launcher vérifie aussi ses propres releases GitHub : le bouton **GitHub** du pied de page devient **Update** quand une nouvelle version du launcher lui-même est sortie. Le clic lance `ports_launcher_updater.bat` (télécharge le dernier build, remplace les fichiers actuels, et relance)
- Interrupteur **Check for Updates** dans Réglages — un seul bouton pour couper toutes les vérifications de mise à jour d'un coup (celles du launcher ET de chaque port), pour qui préfère tout mettre à jour à la main
- Suivi du temps de jeu — le temps cumulé de chaque port apparaît dans son panneau Info, avec un bouton **Reset Game Time** pour remettre le compteur à zéro si besoin
- Désinstallation en un clic, en préservant ta sauvegarde si elle vit dans le dossier même du port ; le visuel de la boîte est téléchargé une fois puis mis en cache localement
- Bouton **Backup Saves** dans Réglages pour exporter en un clic les sauvegardes de tout le catalogue vers un dossier daté (voir [Sauvegardes de jeux](#sauvegardes-de-jeux) pour les deux mécanismes)
- Panneau Info par port : version/tag installé, instructions d'installation (texte sélectionnable/copiable), et liens en un clic vers le site, la page de mods, le dossier d'installation et le(s) dossier(s) de sauvegarde
- Bouton **Select version** dans le panneau Info — choisis parmi les dernières releases GitHub/GitLab et installe-en une autre que la dernière, utile quand la version la plus récente laisse tomber une plateforme dont tu as besoin (ex: un build Windows) ; sert aussi à forcer une mise à jour immédiatement plutôt que d'attendre le prochain Play
- Bouton **Install extras** dans le panneau Info — pour un port dont l'entrée `ports.json` a un lien `extra`, télécharge à la demande ce fichier ou cette archive dans le dossier du port (en écrasant les fichiers de même nom ; une archive est décompressée, un fichier seul est simplement déposé tel quel). Prévu pour des ajouts optionnels qui ne font pas partie des releases du port lui-même — options de lancement toutes faites ou fichiers de configuration prédéfinis, souvent un choix arbitraire. Installer un port ne les récupère jamais tout seul
- Mode « Bibliothèque » plein écran (`Alt+Entrée`) — tous les ports *installés* sous forme de grille de cartes, façon Steam Big Picture
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

Sélectionne un port et ouvre son panneau **Info** (bouton, ou `X` à la manette) pour sa version/tag installée, les instructions d'installation éventuelles de `ports.json` (texte sélectionnable, copiable/collable), et des liens en un clic vers son site, sa page de mods, son dossier d'installation et son dossier de sauvegarde — plus un bouton **Save folder 2** pour les ports avec un second emplacement de sauvegarde indépendant (`save2`). N'importe lequel de ces boutons est simplement désactivé s'il n'existe pas encore (pas installé, le jeu n'a pas encore créé de sauvegarde, ou le port n'a tout simplement pas de second emplacement). `↑`/`↓` (ou la croix directionnelle/le stick de la manette) fait défiler le texte d'instructions quand il est trop long pour tenir ; `←`/`→` déplace la sélection entre les boutons, `Entrée`/`A` active celui qui est en surbrillance.

À côté du texte de version, un bouton **Select version** (ports GitHub/GitLab uniquement) récupère les dernières releases disponibles et permet d'en installer une autre que la dernière — pratique quand la version la plus récente n'a pas de build pour ta plateforme, ou pour simplement revenir en arrière. Il va toujours chercher les infos fraîches sur GitHub/GitLab, donc choisir la plus récente dans la liste permet aussi de forcer une mise à jour immédiatement ; installer une version précise de cette façon désactive aussi l'auto-MAJ pour ce port, pour ne pas se la faire silencieusement remplacer par la dernière release au prochain Play.

Pour un port installé, la ligne affiche aussi **Auto-update: On/Off**, **Favorite executable**, et le temps de jeu, chacun avec son propre bouton juste sous le texte de version/statut — active ou désactive l'auto-MAJ port par port, choisis quel exécutable Play lance directement sans redemander à chaque fois, ou réinitialise le temps de jeu suivi pour ce port. Un port dont l'auto-MAJ est désactivée affiche un bouton **Update** jaune barré à côté de **Play** dans la liste principale, comme rappel qu'il ne se mettra pas à jour tout seul.

À côté, un bouton **Install extras** n'est actif que si l'entrée `ports.json` du port porte un lien `extra` *et* que le port est installé. **Play/Install n'installe jamais que le jeu lui-même** — les extras sont à part, et toujours à activer explicitement depuis ici. Un clic (après une demande de confirmation) télécharge ce fichier ou cette archive directement dans le dossier du port, en écrasant tout fichier de même nom. Une archive (`.zip`, `.7z`, `.tar.gz`, `.rar`) est décompressée dans le dossier ; tout le reste — un `.exe` seul, un `.json`, un fichier de config — est simplement déposé tel quel, jamais décompressé. Prévu pour des ajouts optionnels qui ne font pas partie des releases du port lui-même : des façons supplémentaires de jouer, ou des fichiers de configuration prédéfinis — souvent un choix arbitraire, à prendre comme un confort plutôt que comme une recommandation. Si le lien est mort ou vide, il ne se passe rien et rien n'est touché dans le dossier.

## Sauvegardes de jeux

Ports Launcher gère deux mécanismes de sauvegarde séparés, tous deux basés sur les champs `save`/`save2` de `ports.json`, rangés dans un dossier `Saves Backup` à côté de l'exécutable :

**Préservation automatique** (désinstallation/réinstallation) — désinstaller un port dont la sauvegarde vit dans le dossier d'installation la copie vers `Saves Backup/Pending Restore/<folder>/save_folder/` (ou `.../save_folder2/` pour le second champ) juste avant que le reste du dossier ne soit supprimé, puis une installation ultérieure de ce même port la remet directement en place et supprime cette copie temporaire. Une sauvegarde qui vit ailleurs (ex: sous `%APPDATA%`) n'est de toute façon jamais touchée par une désinstallation, elle y survit déjà d'elle-même. `Pending Restore` n'est jamais un historique : un seul emplacement par port/champ, écrasé à chaque désinstallation — utile à savoir si tu fouilles `Saves Backup/` à la main pour une sauvegarde qui semble avoir disparu en pleine réinstallation, ou si une installation est interrompue et qu'il faut la récupérer manuellement.

**Export manuel** (bouton **Backup Saves**, voir [Réglages](#réglages) juste en dessous) — à la demande, exporte les sauvegardes de tout le catalogue (installé ou non, externe ou locale) vers un dossier daté sous `Saves Backup/Global Backups/<date>/<folder>/`, créé à chaque clic sans jamais toucher aux dossiers datés précédents — un vrai historique de snapshots, contrairement à `Pending Restore`.

## Réglages

Ouvre-le depuis le bouton **◯** de la barre de titre — un menu avec six boutons : **Themes**, **Language**, **Files**, **Library**, **Backup Saves**, et **Check for Updates**.

- **Themes** et **Language** ouvrent tous les deux le même type de sélecteur cherchable de façon floue et en direct. Pour Themes, déplacer la sélection (survol souris, ou `↑`/`↓`/le stick de la manette) prévisualise instantanément sur toute l'appli ; valider réécrit directement dans `themes.json`, et fermer sans valider (`Échap`) revient au thème actif avant l'ouverture. Language change la langue de l'interface immédiatement à la sélection, sans redémarrage.
- **Files** ouvre des raccourcis vers `ports.json`, `ports.local.json`, `state.json` et `themes.json`, grisés si un fichier n'existe pas encore.
- **Library** ouvre directement ce dossier dans l'Explorateur.
- **Backup Saves** lance un export complet des sauvegardes de tout le catalogue vers un dossier daté (voir [Sauvegardes de jeux](#sauvegardes-de-jeux) plus haut), avec une fenêtre de progression pendant la copie.
- **Check for Updates** bascule On/Off directement dans le menu — coupe toutes les vérifications de mise à jour d'un coup (celles du launcher ET de chaque port installé au moment du Play), pour qui préfère tout mettre à jour à la main.

## Outils utiles

Deux outils externes reviennent régulièrement dans les instructions **Required files** de `ports.json`, pour préparer tes propres données de jeu avant de les pointer vers un port :

- **[7-Zip](https://github.com/ip7z/7zip)** — une version minimale (ligne de commande) est fournie avec Ports Launcher, mais uniquement pour extraire en interne les releases des ports ; récupère la version classique/graphique séparément pour décompresser un dump `.7z` (ou autre format compressé) d'un jeu que tu possèdes déjà.
- **[extract-xiso](https://github.com/XboxDev/extract-xiso)** — pas fourni avec Ports Launcher, à récupérer séparément : extrait les fichiers individuels d'un `.iso` Xbox/Xbox 360 ; plusieurs ports Xbox 360 demandent explicitement `extract-xiso.exe` dans leurs instructions pour obtenir leur dossier `assets`.

## Configuration

### `ports.json`

Le catalogue principal, à côté de l'exécutable — jamais embarqué dans le `.exe`, donc lui (et le lanceur lui-même) peuvent être mis à jour indépendamment de n'importe quel port. Pas fait pour être édité à la main : il est remplacé en bloc à chaque mise à jour du catalogue, donc tout ce que tu y ajoutes toi-même se fait silencieusement écraser au prochain rafraîchissement — voir [`ports.local.json`](#portslocaljson) plus bas pour ajouter tes propres ports de façon permanente.

### `ports.local.json`

Ton propre catalogue, à côté de `ports.json` — pour les ports que tu gères toi-même plutôt que d'installer via Ports Launcher : tu crées le dossier sous `Library/`, tu y places les fichiers du jeu à la main, et il apparaît exactement comme n'importe quel autre port (jouable, a un panneau Info, apparaît en mode Bibliothèque une fois « installé »). Même format d'entrée que `ports.json`, sauf que `source` ne s'applique jamais ici — omets-le, ou laisse-le vide. Ce fichier vivant à part, remplacer `ports.json` par une version plus récente du mainteneur ne touche jamais à ce que tu as ajouté ici.

Un `folder` qui entre en collision avec celui d'une entrée de `ports.json` remplace l'entrée officielle par la tienne. Le bouton **Uninstall** se comporte aussi différemment pour un port local : il ne supprime jamais rien — il ouvre le dossier du port dans l'Explorateur à la place, pour que tu gardes le contrôle total de fichiers que Ports Launcher n'a jamais téléchargés lui-même.

Le dépôt fournit un `ports.local.json` avec deux exemples désactivés (`"name"`/`"folder"` mis à `null`, ce qui fait que Ports Launcher les ignore) — copie-en un, remplis de vraies valeurs, et ça devient une vraie entrée.

### `themes.json`

```json
{
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

Même format de catalogue que `themes.json` de [MAGI Launcher](https://github.com/Nyaldee/MAGI-Launcher) — juste les palettes de couleurs elles-mêmes ; quel thème est actif, et toutes les autres préférences d'affichage, vivent dans `state.json` ci-dessous à la place (séparé exprès, pour qu'une mise à jour de `themes.json` par le mainteneur, qui remplace ce fichier en bloc, ne réinitialise jamais tes propres préférences). Change de thème en direct depuis le sélecteur intégré (voir [Réglages](#réglages) plus haut).

### `state.json`

Fichier interne (versions installées/temps de jeu, état de la fenêtre, horodatages des vérifications de mise à jour) que Ports Launcher écrit tout seul — tu ne devrais pas avoir besoin d'y toucher. Tes préférences d'affichage vivent sous sa clé `"ui"` : quel thème est actif, la police, le texte de la barre de recherche, si l'horloge à côté de la barre de recherche est affichée, et la taille/bordure du mode fenêtré — ces mêmes valeurs se mettent aussi à jour en direct et se persistent ici via le sélecteur intégré/`Ctrl+1`...`Ctrl+0`/`Ctrl+-`/`Ctrl+=` (voir [Raccourcis clavier](#raccourcis-clavier)), les éditer à la main ne sert donc qu'à changer ce que le tout premier lancement affiche.

`state.json` est aussi le seul moyen d'augmenter le quota d'API non authentifiée GitHub/GitLab pour les vérifications de mise à jour : ajoute une clé `"github_token"`/`"gitlab_token"` à la main (pas de champ dédié dans l'UI pour l'instant). Ce token est alors stocké en **texte clair**. Ne partage jamais ce fichier, ne l'upload nulle part, et ne le laisse jamais visible sur un stream/partage d'écran — contrairement à `ports.json`/`themes.json`/`ports.local.json`, il peut contenir un identifiant.

## Crédits

- [SteamGridDB](https://www.steamgriddb.com/) pour les visuels de boîte utilisés par les entrées du catalogue
- Les créateurs des recomps/source-ports catalogués dans `ports.json` — sans leur travail, souvent des mois de rétro-ingénierie bénévole, rien de tout ça n'existerait
- [7-Zip](https://github.com/ip7z/7zip) et [extract-xiso](https://github.com/XboxDev/extract-xiso), les deux outils externes utilisés/recommandés pour préparer les fichiers requis (voir [Outils utiles](#outils-utiles))

Construit avec [Claude](https://claude.com) (l'assistant de code IA d'Anthropic).

## Licence

Copyright (C) 2026 Nyaldee. Distribué sous licence [GNU General Public License v3.0](LICENSE) — voir le fichier `LICENSE` pour le texte complet.
