# Ports Launcher

<p align="center">
  <img src="Ports Launcher.jpg" alt="Ports Launcher screenshot">
</p>

*[Read in English](README.md)*

> [!IMPORTANT]
> **Ports Launcher lui-même ne distribue aucun fichier de jeu, ROM, ISO, ni aucun autre asset protégé par le droit d'auteur.** Chaque entrée du catalogue n'installe jamais que le *code* du recomp/source-port — un build open-source publié par le dépôt du projet lui-même. Toute donnée originale de jeu nécessaire reste à fournir toi-même, depuis ta propre copie acquise légalement ; Ports Launcher n'embarque, ne télécharge, n'héberge et ne renvoie jamais vers ces données, nulle part.

Une bibliothèque/installateur léger pour les portages de jeux non-officiels : parcours un catalogue, installe directement depuis une release GitHub/GitLab (ou un lien de téléchargement direct), lance, et garde tout à jour depuis une seule fenêtre. Écrit en Rust et [Slint](https://slint.dev), entièrement navigable au clavier *et* à la manette, rien à installer séparément (un 7-Zip minimal est fourni à côté de l'exécutable pour extraire les releases des ports).

## Fonctionnalités

- Bibliothèque pilotée par catalogue (`ports.json`), plus ton propre catalogue local (`ports.local.json`) pour les ports que tu gères toi-même
- Installation en un clic : télécharge automatiquement le bon asset de release pour ton architecture
- Mise à jour automatique par défaut — désactivable port par port depuis le bouton **Mettre à jour** du panneau Info
- Ports Launcher vérifie aussi ses propres releases GitHub — le bouton **GitHub** du pied de page devient **Mettre à jour** quand une nouvelle version du launcher lui-même est sortie ; les mises à jour sont désactivables, ou peuvent être forcées immédiatement, depuis ◯ Réglages
- Panneau Info par port : version/tag installé, instructions d'installation (texte sélectionnable/copiable), et liens en un clic vers le site, la page de mods, le dossier d'installation et le(s) dossier(s) de sauvegarde
- Bouton **Sélectionner une version** dans le panneau Info — choisis parmi les dernières releases GitHub/GitLab et installe-en une autre que la dernière, ou force une mise à jour immédiatement ; désactive aussi la mise à jour automatique pour ce port, réactivable à tout moment
- Bouton **Installer les extras** dans le panneau Info — installe des pré-configurations optionnelles pour un port quand elles sont disponibles, en écrasant celles déjà en place
- Désinstallation en un clic, en préservant ta sauvegarde si elle vit dans le dossier même du port ; le visuel de la boîte est téléchargé une fois puis mis en cache localement
- Bouton **Copie de sauvegardes** dans ◯ Réglages pour exporter en un clic les sauvegardes de tout le catalogue vers un `.zip` daté (voir [Sauvegardes de jeux](#sauvegardes-de-jeux) pour les deux mécanismes)
- Suivi du temps de jeu — le temps cumulé et la date de dernière partie de chaque port apparaissent dans son panneau Info, avec un bouton **Réinitialiser le temps de jeu** pour remettre le compteur à zéro si besoin
- Le catalogue liste tes ports les plus récemment joués en premier
- Interrupteur **Discord Rich Presence** dans ◯ Réglages (désactivé par défaut) — affiche le jeu auquel tu joues sur ton profil Discord tant qu'il tourne
- Mode « Bibliothèque » plein écran (`Alt+Entrée`) — tous les ports *installés* sous forme de grille de cartes, façon Steam Big Picture
- Navigation manette complète (XInput) *et* navigation clavier complète (flèches, Entrée, Échap) partout dans l'appli, y compris dans chaque dialogue — parcourir, installer, lancer, ouvrir les infos, choisir un fichier/exécutable, et sortir, à la manette comme au clavier
- Plus de 100 thèmes de couleur prêts à l'emploi, changeables en direct depuis le sélecteur intégré à ◯ Réglages avec aperçu instantané
- Icône dans la zone de notification (tray) — fermer la fenêtre principale l'y envoie au lieu de quitter. Clic gauche sur l'icône pour la faire revenir ; clic droit pour un menu avec tes 5 jeux les plus récemment joués (lance directement, comme **Jouer**), **Paramètres**, et **Quitter Ports Launcher**. Un bouton minimiser dédié vit aussi dans la barre de titre, juste à côté de ◯ Réglages

## Clavier et manette

| Touche ⌨️ | Bouton 🎮 | Action |
|---|---|---|
| Taper | — | Filtre le catalogue en direct (recherche floue) |
| `↑` / `↓` ou `Ctrl+W` / `Ctrl+S` | Croix directionnelle / stick gauche | Déplace la sélection haut / bas |
| `←` / `→` ou `Ctrl+A` / `Ctrl+D` | Croix directionnelle / stick gauche | Saute d'une page (10 lignes, liste fenêtrée) / d'une colonne (grille Bibliothèque) |
| `Entrée` | `A` ou `Start` | Installe le port sélectionné s'il n'est pas encore installé, le lance sinon |
| `Shift+Entrée` | — | Ouvre le dossier d'installation du port sélectionné dans l'Explorateur |
| — | `X` | Ouvre le panneau Info du port sélectionné |
| — | `Y` | Ouvre ◯ Réglages |
| `Alt+Entrée` | `Back` | Bascule le mode Bibliothèque plein écran |
| `Ctrl+1`...`Ctrl+9` / `Ctrl+0` | — | Redimensionne le lanceur fenêtré à 10%...90% / 100% de la taille de l'écran, mode fenêtré uniquement |
| `Ctrl+-` / `Ctrl+=` | — | Réduit / agrandit la bordure de 1px, mode fenêtré uniquement |
| `Échap` | `B` | Ferme le dialogue au premier plan s'il y en a un, sort du mode Bibliothèque s'il est actif, sinon ferme le lanceur (`B` seul ne fait que sortir d'un dialogue) |

Branche une manette XInput (façon Xbox) et elle répond immédiatement, sans configuration, juste à côté du clavier. La barre de recherche garde toujours le focus dans la fenêtre principale — chaque touche/bouton ci-dessus y est intercepté directement. Un dialogue ouvert par-dessus (Info, ◯ Réglages, progression d'install, sélecteur de fichier/exécutable...) a son propre focus : les mêmes touches/boutons y déplacent la sélection, `Entrée`/`A` l'active, `Échap`/`B` le ferme (sauf le dialogue de progression d'install, qui ne peut pas être interrompu). Déplacer la sélection à la souris et au clavier/à la manette reste toujours synchronisé — une seule chose est jamais mise en surbrillance à la fois, peu importe comment tu l'y as amenée.

Il est aussi possible de filtrer la recherche par tag pour trouver des jeux. Quelques-uns à connaître : `free` pour les jeux **légalement** gratuits (open-source ou freeware), `quality` pour les ports jugés particulièrement soignés/bien faits, et `online` pour les ports avec multijoueur en ligne. Le reste des tags du catalogue fonctionne pareil — plateforme d'origine (`n64`, `ps1`, `snes`, `360`, `gc`...), genre (`platformer`, `rpg`, `fighting`...), `multiplayer`, `multilingual`, etc.

## Mode Bibliothèque

`Alt+Entrée` bascule vers une grille plein écran de tous les ports actuellement *installés* — comme la bibliothèque Big Picture de Steam. Les ports pas encore installés n'apparaissent que dans la vue liste fenêtrée, jamais en mode Bibliothèque. `Échap` (ou `Alt+Entrée` à nouveau) revient à la vue fenêtrée.

## Panneau Info

Sélectionne un port et ouvre son panneau **Info** (bouton, ou `X` à la manette) pour sa version/tag installée, les instructions d'installation éventuelles (texte sélectionnable, copiable/collable), et des liens en un clic vers son site, sa page de mods, son dossier d'installation et son dossier de sauvegarde — plus un bouton **Dossier de sauvegarde 2** pour les ports avec un second emplacement de sauvegarde indépendant (`save2`). N'importe lequel de ces boutons est simplement désactivé s'il n'existe pas encore (pas installé, le jeu n'a pas encore créé de sauvegarde, ou le port n'a tout simplement pas de second emplacement). `↑`/`↓` (ou la croix directionnelle/le stick de la manette) fait défiler le texte d'instructions quand il est trop long pour tenir ; `←`/`→` déplace la sélection entre les boutons, `Entrée`/`A` active celui qui est en surbrillance.

À côté du texte de version, un bouton **Sélectionner une version** (ports GitHub/GitLab uniquement) récupère les dernières releases disponibles et permet d'en installer une autre que la dernière — pratique quand la version la plus récente n'a pas de build pour ta plateforme, ou pour simplement revenir en arrière. Il va toujours chercher les infos fraîches sur GitHub/GitLab, donc choisir la plus récente dans la liste permet aussi de forcer une mise à jour immédiatement ; installer une version précise de cette façon désactive aussi l'auto-MAJ pour ce port, pour ne pas se la faire silencieusement remplacer par la dernière release au prochain **Jouer**.

Pour un port installé, la ligne affiche aussi **Mise à jour : activée/désactivée**, **Exécutable favori**, la dernière partie et le temps de jeu, chacun avec son propre bouton juste sous le texte de version/statut — active ou désactive l'auto-MAJ port par port, choisis quel exécutable **Jouer** lance directement sans redemander à chaque fois, ou réinitialise le temps de jeu suivi pour ce port. Un port dont l'auto-MAJ est désactivée affiche un bouton **Mettre à jour** jaune barré à côté de **Jouer** dans la liste principale, comme rappel qu'il ne se mettra pas à jour tout seul.

À côté, un bouton **Installer les extras** installe des pré-configurations optionnelles pour le port quand elles sont disponibles, en écrasant celles déjà en place — jamais récupérées automatiquement par **Jouer**/**Installer**, toujours à activer explicitement depuis ici.

## Sauvegardes de jeux

Ports Launcher gère deux mécanismes de sauvegarde séparés, tous deux basés sur les champs `save`/`save2` de `ports.json`, rangés dans un dossier `Saves Backup` à côté de l'exécutable :

**Préservation automatique** (désinstallation/réinstallation) — désinstaller un port dont la sauvegarde vit dans le dossier d'installation la copie vers `Saves Backup/Pending Restore/<folder>/save_folder/` (ou `.../save_folder2/` pour le second champ) juste avant que le reste du dossier ne soit supprimé, puis une installation ultérieure de ce même port la remet directement en place et supprime cette copie temporaire. Une sauvegarde qui vit ailleurs (ex: sous `%APPDATA%`) n'est de toute façon jamais touchée par une désinstallation, elle y survit déjà d'elle-même. `Pending Restore` n'est jamais un historique : un seul emplacement par port/champ, écrasé à chaque désinstallation — utile à savoir si tu fouilles `Saves Backup/` à la main pour une sauvegarde qui semble avoir disparu en pleine réinstallation, ou si une installation est interrompue et qu'il faut la récupérer manuellement.

**Export manuel** (bouton **Copie de sauvegardes**, voir ◯ [Réglages](#réglages) juste en dessous) — à la demande, exporte les sauvegardes de tout le catalogue (installé ou non, externe ou locale) dans `Saves Backup/Global Backups/<date>.zip`. Relancer le même jour écrase le `.zip` du jour avec l'état actuel plutôt que de fusionner dedans, pour qu'un port désinstallé depuis ne traîne pas dedans -- chaque date reste un instantané propre, un vrai historique contrairement à `Pending Restore`.

## Réglages

Ouvre-le depuis le bouton **◯** de la barre de titre — un menu avec **Thèmes**, **Langue**, **Fichiers**, **Bibliothèque**, **Copie de sauvegardes**, **Vérifier les mises à jour**, **Forcer la mise à jour**, et **Discord Rich Presence**.

- **Thèmes** et **Langue** ouvrent tous les deux le même type de sélecteur cherchable de façon floue et en direct. Pour Thèmes, déplacer la sélection (survol souris, ou `↑`/`↓`/le stick de la manette) prévisualise instantanément sur toute l'appli ; valider réécrit directement dans `themes.json`, et fermer sans valider (`Échap`) revient au thème actif avant l'ouverture. Langue change la langue de l'interface immédiatement à la sélection, sans redémarrage.
- **Fichiers** ouvre des raccourcis vers `ports.json`, `ports.local.json`, `state.json` et `themes.json`, sans effet si le fichier n'existe pas encore.
- **Bibliothèque** ouvre directement ce dossier dans l'Explorateur.
- **Copie de sauvegardes** lance un export complet des sauvegardes de tout le catalogue vers un `.zip` daté (voir [Sauvegardes de jeux](#sauvegardes-de-jeux) plus haut), avec une fenêtre de progression pendant la copie.
- **Vérifier les mises à jour** bascule activé/désactivé directement dans le menu — coupe toutes les vérifications de mise à jour d'un coup (celles du launcher ET de chaque port installé au moment du **Jouer**), pour qui préfère tout mettre à jour à la main.
- **Forcer la mise à jour** relance l'updater du launcher tout de suite, sans vérifier au préalable qu'une nouvelle version existe réellement — pour qui ne veut pas attendre la vérification périodique (ou l'a désactivée juste au-dessus).
- **Discord Rich Presence** bascule On/Off directement dans le menu, désactivé par défaut — affiche le jeu auquel tu joues sur ton statut Discord tant qu'il tourne, et s'efface dès sa fermeture.

## Outils utiles

Deux outils externes reviennent régulièrement dans les instructions **Required files** de `ports.json`, pour préparer tes propres données de jeu avant de les pointer vers un port :

- **[7-Zip](https://github.com/ip7z/7zip)** — une version minimale (ligne de commande) est fournie avec Ports Launcher, mais uniquement pour extraire en interne les releases des ports ; récupère la version classique séparément pour gérer des archives au besoin.
- **[extract-xiso](https://github.com/XboxDev/extract-xiso)** — pas fourni avec Ports Launcher, à récupérer séparément : sert à extraire les assets de tes dumps Xbox/Xbox 360 que tu possèdes.

## Configuration

### `ports.json`

Le catalogue principal. Pas fait pour être édité à la main : il est remplacé en bloc à chaque mise à jour du catalogue, donc tout ce que tu y ajoutes toi-même se fait silencieusement écraser au prochain rafraîchissement — voir [`ports.local.json`](#portslocaljson) plus bas pour ajouter tes propres ports de façon permanente.

### `ports.local.json`

Ton propre catalogue, à côté de `ports.json` — pour les ports que tu gères toi-même plutôt que d'installer via Ports Launcher : tu crées le dossier sous `Library/`, tu y places les fichiers du jeu à la main, et il apparaît exactement comme n'importe quel autre port (jouable, a un panneau Info, apparaît en mode Bibliothèque une fois « installé »). Même format d'entrée que `ports.json`, sauf que `source` ne s'applique jamais ici — omets-le, ou laisse-le vide. Ce fichier vivant à part, remplacer `ports.json` par une version plus récente du mainteneur ne touche jamais à ce que tu as ajouté ici.

Un `folder` qui entre en collision avec celui d'une entrée de `ports.json` remplace l'entrée officielle par la tienne. Le bouton **Désinstaller** se comporte aussi différemment pour un port local : il ne supprime jamais rien — il ouvre le dossier du port dans l'Explorateur à la place, pour que tu gardes le contrôle total de fichiers que Ports Launcher n'a jamais téléchargés lui-même.

Le dépôt fournit un `ports.local.json` avec deux exemples désactivés (`"name"`/`"folder"` mis à `null`, ce qui fait que Ports Launcher les ignore) — copie-en un, remplis de vraies valeurs, et ça devient une vraie entrée.

### `state.json`

Le fichier de configuration de Ports Launcher — versions installées/temps de jeu, état de la fenêtre, préférences d'affichage, et horodatages des vérifications de mise à jour, écrit automatiquement au fil de l'utilisation.

Tu peux aussi y ajouter une clé `"github_token"`/`"gitlab_token"` à la main pour augmenter le quota d'API GitHub/GitLab pour les vérifications de mise à jour. Ce token est alors stocké en **texte clair** — ne partage jamais ce fichier, ne l'upload nulle part, et ne le laisse jamais visible sur un stream/partage d'écran.

## Crédits

- [SteamGridDB](https://www.steamgriddb.com/) pour les visuels de boîte utilisés par les entrées du catalogue
- Les créateurs des recomps/source-ports catalogués dans `ports.json` — sans leur travail, souvent des mois de rétro-ingénierie bénévole, rien de tout ça n'existerait
- [7-Zip](https://github.com/ip7z/7zip) et [extract-xiso](https://github.com/XboxDev/extract-xiso), les deux outils externes utilisés/recommandés pour préparer les fichiers requis (voir [Outils utiles](#outils-utiles))

Construit avec [Claude](https://claude.com) (l'assistant de code IA d'Anthropic).

## Licence

Copyright (C) 2026 Nyaldee. Distribué sous licence [GNU General Public License v3.0](LICENSE) — voir le fichier `LICENSE` pour le texte complet.
