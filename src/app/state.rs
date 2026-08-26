//! `AppState` -- état partagé entre les callbacks Slint (souris/clavier) et
//! la cible manette (voir `ui::gamepad_router`) : un seul `Rc` cloné partout
//! plutôt qu'une dizaine de `Rc<RefCell<...>>` séparés, pour que les deux
//! chemins d'entrée appellent exactement la même logique.

use super::cards::build_card_rows;
use super::dialogs::DialogSlot;
use super::events::AppEvent;
use crate::core::models::Port;
use crate::ui::font_sizing::{apply_mode_geometry, compute_mode_geometry, ModeGeometry};
use crate::ui::windows_chrome;
use crate::{AppWindow, Theme};
use slint::{ComponentHandle, Model};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Chemins persistants résolus une fois au démarrage (voir `base_dir()` dans
/// main.rs) -- jamais recalculés en cours de session.
pub(crate) struct AppPaths {
    pub(crate) library_dir: PathBuf,
    pub(crate) cache_dir: PathBuf,
    /// Dossier contenant `ports.json`/`ports.local.json`/`state.json`
    /// (`bdir` dans `main()`) -- pour les raccourcis fichiers du menu
    /// Settings. `themes.json` passe par `themes_path` plutôt que d'être
    /// reconstruit depuis ici.
    pub(crate) config_dir: PathBuf,
    /// Racine "Saves Backup", à côté de "Library" -- contient `Pending
    /// Restore` (slot unique préservé le temps d'une désinstallation/
    /// réinstallation, voir `core::save_backup`) et `Global Backups/<date>`
    /// (export manuel, voir `start_save_backup`). Jamais créée à l'avance :
    /// `core::save_backup::copy_non_empty` ne crée un dossier qu'à sa
    /// première écriture réelle.
    pub(crate) saves_backup_dir: PathBuf,
    /// Chemin de `themes.json` -- persiste immédiatement la taille de
    /// fenêtre/bordure choisie au clavier (voir `set_window_size_percent`/
    /// `adjust_border`), comme `commit_theme` pour le sélecteur de thème.
    pub(crate) themes_path: PathBuf,
}

/// Thème actif : couleurs complètes + couleurs sémantiques fixes + police
/// résolue.
pub(crate) struct ThemeState {
    /// Struct COMPLÈTE (thèmes nommés/thème actif/couleurs appliquées),
    /// contrairement à `semantic` -- nécessaire au sélecteur de thème
    /// (`open_settings_dialog` et `ui::theme::preview_theme`/
    /// `list_theme_names`/`commit_theme` y lisent/écrivent). `apply_theme`
    /// la prend en paramètre plutôt que de lire ce champ, pour rester
    /// appelable au démarrage comme pendant une prévisualisation.
    pub(crate) theme_config: RefCell<crate::ui::theme::ThemeConfig>,
    pub(crate) semantic: crate::ui::theme::SemanticColors,
    /// Résolue une fois au démarrage (`ui::theme::resolve_font_family`) --
    /// poussée à la fenêtre principale ET, via `apply_dialog_theme!`, à
    /// chaque dialogue : une seule source de vérité pour toute l'appli.
    pub(crate) font_family: String,
}

/// Géométrie des deux modes (fenêtré/plein écran) + réglages qui la
/// pilotent -- tout ce que `toggle_fullscreen`/`recompute_normal_mode`
/// lisent ou écrivent.
pub(crate) struct WindowGeometry {
    /// `Cell` : modifiable en direct par Ctrl+-/Ctrl+= (voir
    /// `adjust_border`), `AppState` étant partagé via `Rc` entre callbacks
    /// Slint et donc jamais accessible en `&mut`.
    pub(crate) border_width: Cell<i32>,
    /// Fraction 0.05-1.0 de la taille d'écran visée par le mode FENÊTRÉ --
    /// `Cell` pour la même raison que `border_width` (Ctrl+chiffres, voir
    /// `set_window_size_percent`). Sans objet en plein écran, qui occupe
    /// l'écran entier par définition.
    pub(crate) window_width_fraction: Cell<f64>,
    /// Facteur d'échelle DPI réel, initialisé juste après le premier
    /// `show()`. `recompute_normal_mode`/`toggle_fullscreen` le remettent à
    /// jour depuis `window.scale_factor()` -- seule façon dont un changement
    /// d'échelle Windows en cours de session se répercute, sans Timer dédié.
    /// `apply_dialog_theme!` le relit à chaque ouverture de dialogue.
    pub(crate) scale: Cell<f32>,
    /// Géométrie du mode FENÊTRÉ -- calculée au démarrage, recalculée
    /// UNIQUEMENT sur une demande explicite de redimensionnement clavier
    /// (voir `set_window_size_percent`/`adjust_border`). `toggle_fullscreen`
    /// la relit telle quelle sans jamais la recalculer, pour ne pas "bouger"
    /// d'un aller-retour plein écran à l'autre.
    pub(crate) normal_mode: RefCell<ModeGeometry>,
    /// Géométrie du mode plein écran -- les raccourcis de redimensionnement
    /// ne s'y appliquent pas (garde `!root.big-mode` dans app-window.slint).
    /// `RefCell` quand même : `toggle_fullscreen` la recalcule à chaque
    /// ENTRÉE en plein écran avec le `scale` du moment.
    pub(crate) fullscreen_mode: RefCell<ModeGeometry>,
}

/// État de la grille plein écran (ports affichés, sélection, cache
/// d'images) + le double-clic qui en dépend.
pub(crate) struct GridNav {
    /// Recalculé à chaque entrée en plein écran (voir `recompute_grid_columns`)
    /// -- figé au démarrage causait une grille mal centrée si l'utilisateur
    /// déplaçait la fenêtre vers un moniteur de résolution différente avant
    /// de basculer.
    pub(crate) grid_columns: Cell<usize>,
    /// Ports affichés dans la grille plein écran -- reconstruits à chaque
    /// entrée en plein écran et après chaque install/désinstall (voir
    /// rebuild_grid) ; `refresh_grid_selection` se contente de relire cette
    /// liste pour recolorer la sélection.
    pub(crate) displayed_installed: RefCell<Vec<Port>>,
    pub(crate) grid_selected: Cell<(usize, usize)>,
    /// Faux entre un `card-unhovered` (souris sortie de la grille) et le
    /// prochain survol/navigation -- masque seulement la surbrillance dans
    /// `build_card_rows` sans toucher `grid_selected`, pour que la
    /// navigation clavier/manette reprenne là où le survol l'avait laissée
    /// plutôt qu'en (0, 0).
    pub(crate) grid_mouse_active: Cell<bool>,
    /// Dernier clic souris sur une carte en plein écran (position, instant)
    /// -- voir `on_card_activated` : un clic isolé ne fait que sélectionner,
    /// lancer exige un vrai double-clic dans le délai configuré par Windows.
    /// `None` tant qu'aucun clic n'a eu lieu, ou juste après qu'un
    /// double-clic ait été consommé (un 3e clic rapide ne redéclenche rien).
    pub(crate) last_card_click: Cell<Option<((usize, usize), Instant)>>,
    /// `windows_chrome::double_click_time_ms()`, lu une fois au démarrage --
    /// le délai réel configuré dans Windows, jamais une valeur en dur qui
    /// ignorerait les réglages d'accessibilité de l'utilisateur.
    pub(crate) double_click_ms: u32,
    /// Cache mémoire des jaquettes déjà décodées (clé : folder, voir
    /// load_cached_card_image) -- une entrée n'est invalidée qu'après un
    /// (ré)install (voir AppEvent::InstallDone), une jaquette ne changeant
    /// pas spontanément.
    pub(crate) card_image_cache: RefCell<HashMap<String, slint::Image>>,
}

/// État de la liste fenêtrée (ports affichés, sélection, recherche en
/// cours).
pub(crate) struct WindowedNav {
    /// Ports actuellement affichés dans la liste fenêtrée (après filtrage
    /// par la recherche) -- dans le même ordre que le modèle Slint.
    pub(crate) displayed_windowed: RefCell<Vec<Port>>,
    pub(crate) windowed_selected: Cell<usize>,
    /// Dernière recherche tapée -- retenue pour reconstruire la liste après
    /// un install/uninstall/vérif de MAJ sans perdre le filtre en cours
    /// (voir refresh_current_view).
    pub(crate) search_query: RefCell<String>,
}

/// Bookkeeping des installs/lancements en cours -- ce qui tourne
/// actuellement, depuis quand, et ce qui doit se lancer automatiquement une
/// fois son install terminée.
pub(crate) struct InstallRuntime {
    /// Clés (`Port::key`) actuellement en cours d'installation -- ignore les
    /// activations répétées pendant qu'un install tourne déjà.
    pub(crate) installing: RefCell<HashSet<String>>,
    /// Process lancé pour chaque port en cours d'exécution (voir
    /// `is_port_running`) -- évite de relancer un port déjà ouvert et
    /// interdit désinstallation/mise à jour pendant qu'il tourne :
    /// `remove_dir_all`/l'extraction échoueraient sur des fichiers
    /// verrouillés par Windows, potentiellement après avoir déjà supprimé
    /// une partie de l'arborescence.
    pub(crate) running_processes: RefCell<HashMap<String, crate::core::launch::LaunchedProcess>>,
    /// Horodatage du lancement -- ou du dernier checkpoint, voir
    /// `checkpoint_playtime` -- de chaque entrée de `running_processes`.
    /// Séparé plutôt qu'ajouté à `LaunchedProcess` (type partagé de
    /// `core::launch`, sans notion de temps de jeu). Consulté par
    /// `record_playtime` (fin de partie), `checkpoint_playtime` (persistance
    /// périodique anti-crash, remet aussi cet horodatage à `Instant::now()`
    /// pour ne jamais recompter le même intervalle deux fois) et
    /// l'affichage en direct du temps de jeu dans InfoDialog (lecture seule,
    /// voir `AppState::info_dialog_port_key`).
    pub(crate) launch_started_at: RefCell<HashMap<String, Instant>>,
    /// Clés en attente d'un lancement automatique une fois leur
    /// `AppEvent::InstallDone` reçu -- alimenté uniquement par
    /// `launch_with_update_check` (auto-install déclenché par Play, voir
    /// `AppEvent::PlayUpdateChecked`), jamais par un install/update "normal"
    /// (bouton Install, Change version), qui ne doit jamais lancer le jeu
    /// tout seul.
    pub(crate) pending_launch_after_install: RefCell<HashSet<String>>,
    /// Vrai entre la minimisation faite par `launch_executable` au lancement
    /// d'un jeu en plein écran et la remontée par `poll_app_events` -- sert
    /// à distinguer NOTRE minimisation de celle d'un raccourci Windows,
    /// qu'il ne faudrait surtout pas annuler de force.
    pub(crate) minimized_for_game: Cell<bool>,
}

/// Navigation clavier/manette dans le dialogue actuellement ouvert -- voir
/// `app::gamepad_target::DialogGamepadTarget`.
pub(crate) struct DialogNav {
    pub(crate) dialogs: RefCell<DialogSlot>,
    /// Option actuellement en surbrillance manette dans un ListPickerDialog
    /// ouvert -- sans objet tant qu'aucun picker n'est affiché.
    pub(crate) picker_index: Cell<i32>,
    /// Bouton actuellement en surbrillance manette dans un InfoDialog ouvert
    /// (Website=0/Mods=1/Game folder=2/Save folder=3/Save folder 2=4/Change
    /// version=5/Favorite executable=6/Update=7/Reset Game Time=8, voir
    /// InfoDialog.selected-index) -- sans objet tant qu'aucun InfoDialog
    /// n'est affiché.
    pub(crate) info_nav_index: Cell<i32>,
    /// Bouton en surbrillance manette/clavier dans un ConfirmDialog ouvert
    /// (0 = confirmer, 1 = annuler, voir ConfirmDialog.selected-index) --
    /// sans objet tant qu'aucun n'est affiché.
    pub(crate) confirm_nav_index: Cell<i32>,
    /// Bouton en surbrillance manette/clavier dans un ErrorDialog ouvert
    /// (0 = réinstaller, 1 = infos, voir ErrorDialog.selected-index) -- sans
    /// objet tant qu'aucun n'est affiché.
    pub(crate) error_nav_index: Cell<i32>,
    /// Clé du port dont l'InfoDialog est actuellement affiché, si InfoDialog
    /// est bien le dialogue ouvert -- `None` sinon (voir close_current_dialog,
    /// seul point qui l'efface). Sert UNIQUEMENT à rafraîchir en direct
    /// "Playtime: {}" pendant qu'une partie tourne (voir poll_app_events) :
    /// sans ce lien, aucun moyen de savoir depuis le timer 100ms QUEL port
    /// afficher sans reparcourir tout `running_processes`.
    pub(crate) info_dialog_port_key: RefCell<Option<String>>,
}

pub(crate) struct AppState {
    pub(crate) window: slint::Weak<AppWindow>,
    pub(crate) state: RefCell<crate::core::state::StateManager>,
    /// `RefCell` : un rafraîchissement distant réussi peut remplacer le
    /// catalogue en cours de session (voir AppEvent::RemoteCatalogFetched).
    pub(crate) catalog: RefCell<Vec<Port>>,
    pub(crate) paths: AppPaths,
    pub(crate) theme: ThemeState,
    pub(crate) window_geometry: WindowGeometry,
    pub(crate) grid_nav: GridNav,
    pub(crate) windowed_nav: WindowedNav,
    pub(crate) install_runtime: InstallRuntime,
    pub(crate) dialog_nav: DialogNav,
    pub(crate) events: Arc<Mutex<Vec<AppEvent>>>,
    /// Vrai uniquement sous `--visual-stress-test` -- fait bifurquer
    /// start_install et launch_executable vers un chemin 100% local, sans
    /// téléchargement ni lancement de process réel.
    pub(crate) stress_test: bool,
}

impl AppState {
    pub(crate) fn window(&self) -> AppWindow {
        self.window.unwrap()
    }

    /// Le port en surbrillance, quel que soit le mode affiché -- source
    /// unique de vérité pour "sur quoi agit Entrée/A/clic".
    pub(crate) fn current_selected_port(&self) -> Option<Port> {
        if self.window().get_big_mode() {
            let (row, col) = self.grid_nav.grid_selected.get();
            let idx = row * self.grid_nav.grid_columns.get().max(1) + col;
            self.grid_nav.displayed_installed.borrow().get(idx).cloned()
        } else {
            self.windowed_nav.displayed_windowed.borrow().get(self.windowed_nav.windowed_selected.get()).cloned()
        }
    }

    /// Reconstruit la liste filtrée/triée pour `query` -- conserve la
    /// sélection sur le MÊME port (par `Port::key`) s'il est toujours
    /// affiché après filtrage, sinon repart du premier élément.
    pub(crate) fn rebuild_windowed(&self, query: &str) {
        *self.windowed_nav.search_query.borrow_mut() = query.to_string();
        let previously_selected_key =
            self.windowed_nav.displayed_windowed.borrow().get(self.windowed_nav.windowed_selected.get()).map(|p| p.key().to_string());
        let catalog = self.catalog.borrow();
        let pool: Vec<&Port> = catalog.iter().collect();
        let filtered = crate::core::search::filter_and_sort(&pool, query);
        let displayed: Vec<Port> = filtered.iter().map(|p| (*p).clone()).collect();
        let new_index = previously_selected_key.and_then(|k| displayed.iter().position(|p| p.key() == k)).unwrap_or(0);
        *self.windowed_nav.displayed_windowed.borrow_mut() = displayed;
        self.windowed_nav.windowed_selected.set(new_index);
        self.rebuild_ports_model();
    }

    /// Reconstruit ENTIÈREMENT le modèle `ports` -- uniquement quand la
    /// LISTE a changé (recherche, install/désinstall), jamais pour un simple
    /// changement de sélection (voir push_selected_index) : la ligne
    /// surlignée se déduit d'un `index == root.selected-index` côté .slint,
    /// et remplacer tout le modèle à chaque survol souris perturberait le
    /// défilement à la molette en cours.
    pub(crate) fn rebuild_ports_model(&self) {
        let displayed = self.windowed_nav.displayed_windowed.borrow();
        let refs: Vec<&Port> = displayed.iter().collect();
        let items = crate::to_port_items(&refs, &self.paths.library_dir, &self.state.borrow());
        self.window().set_ports(slint::ModelRc::new(slint::VecModel::from(items)));
        self.push_selected_index();
    }

    /// Pousse UNIQUEMENT l'index sélectionné -- jamais le modèle `ports`
    /// lui-même, voir rebuild_ports_model.
    pub(crate) fn push_selected_index(&self) {
        let len = self.windowed_nav.displayed_windowed.borrow().len();
        let window = self.window();
        // -1 sur une liste vide, jamais 0 : 0 ferait défiler vers une
        // première ligne qui n'existe pas.
        window.set_selected_index(if len == 0 { -1 } else { self.windowed_nav.windowed_selected.get() as i32 });
    }

    pub(crate) fn move_windowed_selection(&self, dy: i32) {
        let len = self.windowed_nav.displayed_windowed.borrow().len();
        if len == 0 {
            return;
        }
        let current = self.windowed_nav.windowed_selected.get() as i32;
        let next = (current + dy).clamp(0, len as i32 - 1) as usize;
        if next != self.windowed_nav.windowed_selected.get() {
            self.windowed_nav.windowed_selected.set(next);
            self.push_selected_index();
            self.trigger_scroll();
        }
    }

    /// Bascule `scroll-trigger` pour ramener la sélection dans la vue --
    /// appelée UNIQUEMENT depuis la navigation clavier/manette, jamais
    /// depuis le survol souris : un survol pendant un défilement à la
    /// molette ne doit pas l'interrompre.
    pub(crate) fn trigger_scroll(&self) {
        let window = self.window();
        window.set_scroll_trigger(!window.get_scroll_trigger());
    }

    /// Reconstruit la liste des ports installés + la grille. À l'entrée en
    /// plein écran, `preserve_selection: false` repart de (0, 0) ; après un
    /// install/uninstall grille affichée, `true` retrouve le même port par
    /// clé s'il est toujours présent, comme `rebuild_windowed`. Filtrée par
    /// `search_query`, les deux modes partageant la même requête.
    pub(crate) fn rebuild_grid(&self, preserve_selection: bool) {
        let query = self.windowed_nav.search_query.borrow().clone();
        // Vérité disque via `installer::is_installed`, pas `state.json` (qui
        // ne fait que du bookkeeping de tag/date) -- reflète l'état réel de
        // `Library` même si `state.json` est périmé ou absent. Filtre sur
        // des références dans `self.catalog` : un seul clone final.
        let catalog = self.catalog.borrow();
        let installed_refs: Vec<&Port> =
            catalog.iter().filter(|p| crate::core::installer::is_installed(p, &self.paths.library_dir)).collect();
        let installed: Vec<Port> =
            crate::core::search::filter_and_sort(&installed_refs, &query).iter().map(|p| (*p).clone()).collect();
        let columns = self.grid_nav.grid_columns.get().max(1);
        let previously_selected_key = if preserve_selection {
            let (row, col) = self.grid_nav.grid_selected.get();
            self.grid_nav.displayed_installed.borrow().get(row * columns + col).map(|p| p.key().to_string())
        } else {
            None
        };
        let new_flat = previously_selected_key.and_then(|k| installed.iter().position(|p| p.key() == k)).unwrap_or(0);
        self.grid_nav.grid_selected.set((new_flat / columns, new_flat % columns));
        // Reconstruction complète, pas un simple survol -- la surbrillance
        // est toujours affichée dans ce cas (voir grid_mouse_active).
        self.grid_nav.grid_mouse_active.set(true);
        self.window().set_card_rows(slint::ModelRc::new(slint::VecModel::from(build_card_rows(
            &self.grid_nav.card_image_cache,
            &installed,
            &self.paths.cache_dir,
            self.grid_nav.grid_columns.get(),
            Some(self.grid_nav.grid_selected.get()),
        ))));
        *self.grid_nav.displayed_installed.borrow_mut() = installed;
    }

    pub(crate) fn enter_fullscreen(&self) {
        self.recompute_grid_columns();
        self.rebuild_grid(false);
    }

    /// Recalcule `grid_columns` depuis la zone de travail SOUS LE CURSEUR et
    /// l'échelle courante (déjà rafraîchie par `compute_live_mode`, appelé
    /// juste avant par `toggle_fullscreen`) -- sinon un changement de
    /// moniteur avant un passage en plein écran laisserait la grille sur un
    /// nombre de colonnes périmé (le centrage dans card-grid.slint suppose
    /// qu'il correspond exactement à ce qui est affiché).
    fn recompute_grid_columns(&self) {
        let (_, _, screen_w, _) = windows_chrome::work_area_under_cursor();
        let grid_available_width = screen_w as f32 / self.window_geometry.scale.get();
        let columns = crate::core::grid::compute_grid_columns(grid_available_width);
        self.grid_nav.grid_columns.set(columns);
        self.window().set_grid_columns(columns as i32);
    }

    /// Bascule la surbrillance d'UNE carte dans le modèle déjà affiché --
    /// jamais un rebuild (voir build_card_rows/rebuild_grid, qui eux
    /// reconstruisent tout). `cards` reste le même `ModelRc` que celui déjà
    /// dans `card_rows` : le modifier via `set_row_data` met donc directement
    /// à jour l'affichage sans recréer aucun `CardItem`/`ModelRc`.
    fn set_card_highlight(window: &AppWindow, pos: Option<(usize, usize)>, selected: bool) {
        let Some((row, col)) = pos else { return };
        let Some(card_row) = window.get_card_rows().row_data(row) else { return };
        let Some(mut item) = card_row.cards.row_data(col) else { return };
        if item.selected != selected {
            item.selected = selected;
            card_row.cards.set_row_data(col, item);
        }
    }

    /// `previous` -- position (et visibilité) de la surbrillance AVANT ce
    /// changement, à effacer ; `grid_selected`/`grid_mouse_active`
    /// donnent la nouvelle carte à surligner. Les deux mutations touchent au
    /// plus 2 `CardItem`, jamais toute la grille.
    pub(crate) fn refresh_grid_selection(&self, previous: Option<(usize, usize)>) {
        let window = self.window();
        let current = self.grid_nav.grid_mouse_active.get().then(|| self.grid_nav.grid_selected.get());
        if previous != current {
            Self::set_card_highlight(&window, previous, false);
            Self::set_card_highlight(&window, current, true);
        }
        let displayed = self.grid_nav.displayed_installed.borrow();
        // Garde la ligne sélectionnée visible (voir le `changed
        // selected-row-local` sur card-list dans card-grid.slint) -- -1 si
        // la bibliothèque est vide.
        window.set_grid_selected_row(if displayed.is_empty() { -1 } else { self.grid_nav.grid_selected.get().0 as i32 });
    }

    pub(crate) fn move_grid_selection(&self, dx: i32, dy: i32) {
        let len = self.grid_nav.displayed_installed.borrow().len();
        let columns = self.grid_nav.grid_columns.get().max(1);
        // Arithmétique dans core::grid::next_grid_position -- stress-testée
        // là-bas, indépendamment de toute fenêtre réelle.
        let Some(next) = crate::core::grid::next_grid_position(self.grid_nav.grid_selected.get(), dx, dy, columns, len) else {
            return;
        };
        if next != self.grid_nav.grid_selected.get() {
            let previous = self.grid_nav.grid_mouse_active.get().then(|| self.grid_nav.grid_selected.get());
            self.grid_nav.grid_selected.set(next);
            // Ré-affiche la surbrillance si la souris l'avait effacée en
            // sortant de la grille (voir grid_mouse_active).
            self.grid_nav.grid_mouse_active.set(true);
            self.refresh_grid_selection(previous);
            self.trigger_scroll();
        }
    }

    /// Recharge la vue actuellement affichée (liste OU grille selon
    /// big-mode) après tout install/uninstall/vérif de MAJ. La vue MASQUÉE
    /// reste périmée jusqu'au prochain basculement, qui la reconstruit
    /// intégralement (voir toggle_fullscreen).
    pub(crate) fn refresh_current_view(&self) {
        if self.window().get_big_mode() {
            self.rebuild_grid(true);
        } else {
            let query = self.windowed_nav.search_query.borrow().clone();
            self.rebuild_windowed(&query);
        }
    }

    /// Relit `window.scale_factor()` (lecture stable mise en cache par
    /// Slint), la pousse à `Theme` et `window_geometry.scale`, et calcule la
    /// géométrie du mode demandé -- partagé par `toggle_fullscreen` et
    /// `recompute_normal_mode`, seuls points qui recalculent une géométrie
    /// suite à une action utilisateur.
    fn compute_live_mode(&self, big_mode: bool) -> ModeGeometry {
        let window = self.window();
        let scale = window.window().scale_factor();
        window.global::<Theme>().set_scale_factor(scale);
        self.window_geometry.scale.set(scale);
        compute_mode_geometry(
            &self.theme.font_family,
            windows_chrome::work_area_under_cursor(),
            scale,
            big_mode,
            self.window_geometry.window_width_fraction.get(),
            self.window_geometry.border_width.get(),
        )
    }

    /// Bascule entre les deux géométries de mode. Les DEUX sont recalculées
    /// à chaque bascule (voir `compute_live_mode`) : sans ça, un changement
    /// d'échelle Windows survenu pendant que l'appli tourne ne serait capté
    /// que par le mode qu'on est en train d'ACTIVER, laissant l'autre
    /// géométrie périmée -- désaccord fenêtre/contenu à la bascule suivante
    /// (petite fenêtre, contenu à l'ancienne échelle, ou l'inverse).
    pub(crate) fn toggle_fullscreen(&self) {
        let now_big = !self.state.borrow().fullscreen;
        self.state.borrow_mut().set_fullscreen(now_big);
        let mode = self.compute_live_mode(now_big);
        let window = self.window();
        if now_big {
            *self.window_geometry.fullscreen_mode.borrow_mut() = mode;
            apply_mode_geometry(&window, &self.window_geometry.fullscreen_mode.borrow());
        } else {
            *self.window_geometry.normal_mode.borrow_mut() = mode;
            apply_mode_geometry(&window, &self.window_geometry.normal_mode.borrow());
        }
        window.set_big_mode(now_big);
        if now_big {
            self.enter_fullscreen();
        } else {
            let query = self.windowed_nav.search_query.borrow().clone();
            self.rebuild_windowed(&query);
        }
    }

    /// Détecte un changement d'échelle Windows survenu PENDANT que l'appli
    /// tourne (fenêtre déplacée vers un autre moniteur, réglage système
    /// changé...) et réapplique la géométrie du mode actuellement affiché --
    /// sans ça, la fenêtre restait à l'ancienne taille physique jusqu'à ce
    /// que l'utilisateur force un recalcul à la main (Ctrl+chiffre, bascule
    /// plein écran). Appelée à chaque tick du Timer d'évènements (voir
    /// poll_app_events) : `scale_factor()` est une lecture bon marché (mise
    /// en cache par Slint, voir `compute_live_mode`), comparée à la
    /// dernière valeur connue -- aucun recalcul si rien n'a changé.
    pub(crate) fn refresh_geometry_if_scale_changed(&self) {
        let window = self.window();
        if window.window().scale_factor() == self.window_geometry.scale.get() {
            return;
        }
        let big_mode = window.get_big_mode();
        let mode = self.compute_live_mode(big_mode);
        if big_mode {
            *self.window_geometry.fullscreen_mode.borrow_mut() = mode;
            apply_mode_geometry(&window, &self.window_geometry.fullscreen_mode.borrow());
        } else {
            *self.window_geometry.normal_mode.borrow_mut() = mode;
            apply_mode_geometry(&window, &self.window_geometry.normal_mode.borrow());
        }
    }

    /// Recalcule `normal_mode` à partir des `window_width_fraction`/
    /// `border_width` courants et la réapplique (voir `compute_live_mode`).
    fn recompute_normal_mode(&self) {
        *self.window_geometry.normal_mode.borrow_mut() = self.compute_live_mode(false);
        // Rien à réappliquer en plein écran : cette géométrie n'est affichée
        // qu'à la sortie, et toggle_fullscreen la recalculera à nouveau à ce
        // moment-là de toute façon.
        if !self.window().get_big_mode() {
            let window = self.window();
            apply_mode_geometry(&window, &self.window_geometry.normal_mode.borrow());
        }
    }

    /// Ctrl+1..9/0 (voir app-window.slint) : passe la taille de fenêtre à
    /// `percent` (10..100) et la persiste immédiatement dans state.json.
    pub(crate) fn set_window_size_percent(&self, percent: i32) {
        let new_fraction = (percent as f64 / 100.0).clamp(0.05, 1.0);
        // Sans ce garde-fou, marteler Ctrl+1 (déjà à 10%) ou Ctrl+0 (déjà à
        // 100%) relance à chaque frappe tout le cycle recalcul de
        // géométrie/repositionnement/écriture disque pour rien.
        if new_fraction == self.window_geometry.window_width_fraction.get() {
            return;
        }
        self.window_geometry.window_width_fraction.set(new_fraction);
        self.recompute_normal_mode();
        self.state.borrow_mut().set_window_size(percent);
    }

    /// Ctrl+-/Ctrl+= (voir app-window.slint) : ajuste l'épaisseur de bordure
    /// de `delta` px et la persiste tout de suite, même principe que
    /// `set_window_size_percent` -- y compris le même garde-fou contre un
    /// no-op (bordure déjà à 0 ou déjà à sa borne haute).
    pub(crate) fn adjust_border(&self, delta: i32) {
        let new_border = (self.window_geometry.border_width.get() + delta).clamp(0, 100);
        if new_border == self.window_geometry.border_width.get() {
            return;
        }
        self.window_geometry.border_width.set(new_border);
        // recompute_normal_mode ne pousse que géométrie et polices ;
        // l'épaisseur visuelle de la bordure vient de Theme.border-width
        // (voir border-px dans app-window.slint), un global que seul
        // apply_theme pousse au démarrage. Sans ce push, il faudrait
        // redémarrer pour voir la nouvelle valeur prendre effet.
        self.window().global::<Theme>().set_border_width(new_border);
        self.recompute_normal_mode();
        self.state.borrow_mut().set_border(new_border);
    }
}
