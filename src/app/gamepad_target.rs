//! Cibles manette/clavier (voir `ui::gamepad_router`) : le dialogue
//! actuellement ouvert prend toujours le dessus sur la fenêtre principale.

use super::dialogs::DialogSlot;
use super::install_launch::{activate_selection, show_info_for_current_selection};
use super::state::AppState;
use crate::ui::gamepad_router::GamepadTarget;
use crate::{InfoDialog, PAGE_ROWS};
use slint::{ComponentHandle, Model};
use std::cell::RefCell;
use std::rc::Rc;

/// Cible manette générique pour n'importe quel dialogue ouvert -- relit
/// `app.dialog_nav.dialogs` à chaque appel plutôt que de garder un type par dialogue :
/// un seul dialogue est ouvert à la fois (voir DialogSlot). Les actions
/// passent par les MÊMES callbacks Slint que la souris
/// (`invoke_xxx_requested`), pour n'avoir qu'un chemin de code par action.
pub(crate) struct DialogGamepadTarget {
    pub(crate) app: Rc<AppState>,
}

/// Clone fort (voir `ComponentHandle::clone_strong`) du dialogue dans
/// `app.dialog_nav.dialogs`, ou `None` s'il n'y en a pas ou si c'est `Progress` (jamais
/// fermable/activable au clavier-manette). Le clone DOIT sortir du scope du
/// `.borrow()` avant que l'appelant invoque quoi que ce soit dessus -- voir
/// `activate_selection`.
fn cloned_dialog(app: &AppState) -> Option<DialogSlot> {
    match &*app.dialog_nav.dialogs.borrow() {
        DialogSlot::Message(d) => Some(DialogSlot::Message(d.clone_strong())),
        DialogSlot::Confirm(d) => Some(DialogSlot::Confirm(d.clone_strong())),
        DialogSlot::Error(d) => Some(DialogSlot::Error(d.clone_strong())),
        DialogSlot::Picker(d) => Some(DialogSlot::Picker(d.clone_strong())),
        DialogSlot::Info(d) => Some(DialogSlot::Info(d.clone_strong())),
        DialogSlot::SearchList(d) => Some(DialogSlot::SearchList(d.clone_strong())),
        DialogSlot::Progress(_) | DialogSlot::None => None,
    }
}

/// [Website, Mods website, Game folder, Save folder, Save folder 2, Change
/// version, Favorite executable, Update, Reset Game Time] activés -- même
/// ordre que InfoDialog.selected-index (voir dialogs.slint).
fn info_nav_enabled(d: &InfoDialog) -> [bool; 9] {
    [
        d.get_website_enabled(),
        d.get_mods_enabled(),
        d.get_game_folder_enabled(),
        d.get_save_folder_enabled(),
        d.get_save_folder2_enabled(),
        d.get_change_version_enabled(),
        d.get_favorite_exe_enabled(),
        d.get_update_toggle_enabled(),
        d.get_reset_playtime_enabled(),
    ]
}

impl GamepadTarget for DialogGamepadTarget {
    // Passe par `invoke_close_requested()` plutôt que d'appeler
    // `close_current_dialog` directement : le menu Settings a sa propre
    // logique dans `close-requested` (annuler l'effet visuel d'une
    // prévisualisation de thème non confirmée, voir `open_settings_dialog`),
    // et B doit suivre exactement le même chemin que la croix/Échap. Même
    // clone-avant-invoke que `activate_selection`, même raison.
    fn reject(&self) {
        // ProgressDialog n'est pas fermable (interrompre un install
        // laisserait le port bloqué "en cours d'installation") -- B n'y fait
        // rien, comme son bouton fermer désactivé.
        match cloned_dialog(&self.app) {
            Some(DialogSlot::Message(d)) => d.invoke_close_requested(),
            Some(DialogSlot::Confirm(d)) => d.invoke_close_requested(),
            Some(DialogSlot::Error(d)) => d.invoke_close_requested(),
            Some(DialogSlot::Picker(d)) => d.invoke_close_requested(),
            Some(DialogSlot::Info(d)) => d.invoke_close_requested(),
            Some(DialogSlot::SearchList(d)) => d.invoke_close_requested(),
            _ => {}
        }
    }

    // Le clone (léger -- même poignée Rc sous-jacente) DOIT sortir du scope
    // du `.borrow()` AVANT d'appeler `invoke_xxx` : ce callback rejoue la
    // logique du clic souris, qui referme le dialogue via
    // `app.dialog_nav.dialogs.borrow_mut()`. Un `match &*self.app.dialog_nav.dialogs.borrow() {
    // ... => d.invoke_xxx() }` garderait l'emprunt actif pendant tout le
    // bras de match et paniquerait ("already borrowed").
    fn activate_selection(&self) {
        match cloned_dialog(&self.app) {
            // Pas de bouton OK (voir dialogs.slint) -- A/Entrée ferme
            // directement, comme la croix/Échap.
            Some(DialogSlot::Message(d)) => d.invoke_close_requested(),
            // Selon le bouton en surbrillance (voir move_selection) --
            // jamais "confirmer" par défaut pour une action destructive.
            Some(DialogSlot::Confirm(d)) => {
                if self.app.dialog_nav.confirm_nav_index.get() == 0 {
                    d.invoke_confirmed();
                } else {
                    d.invoke_close_requested();
                }
            }
            // Selon le bouton en surbrillance (voir move_selection) --
            // jamais "réinstaller" par défaut, même logique que Confirm.
            Some(DialogSlot::Error(d)) => {
                if self.app.dialog_nav.error_nav_index.get() == 0 {
                    d.invoke_reinstall_requested();
                } else {
                    d.invoke_info_requested();
                }
            }
            Some(DialogSlot::Picker(d)) => d.invoke_item_selected(self.app.dialog_nav.picker_index.get()),
            Some(DialogSlot::SearchList(d)) => d.invoke_item_selected(self.app.dialog_nav.picker_index.get()),
            // Rien ne se passe si le bouton en surbrillance est désactivé
            // (ex: pas de site web renseigné pour ce port).
            Some(DialogSlot::Info(d)) if info_nav_enabled(&d)[self.app.dialog_nav.info_nav_index.get() as usize] => {
                match self.app.dialog_nav.info_nav_index.get() {
                    0 => d.invoke_website_requested(),
                    1 => d.invoke_mods_requested(),
                    2 => d.invoke_game_folder_requested(),
                    3 => d.invoke_save_folder_requested(),
                    4 => d.invoke_save_folder2_requested(),
                    5 => d.invoke_change_version_requested(),
                    6 => d.invoke_favorite_exe_requested(),
                    7 => d.invoke_update_toggle_requested(),
                    _ => d.invoke_reset_playtime_requested(),
                }
            }
            _ => {}
        }
    }

    fn show_info_for_selection(&self) {
        let error_dialog = match &*self.app.dialog_nav.dialogs.borrow() {
            DialogSlot::Error(d) => Some(d.clone_strong()),
            _ => None,
        };
        if let Some(d) = error_dialog {
            d.invoke_info_requested();
        }
    }

    fn move_selection(&self, dx: i32, dy: i32) {
        match &*self.app.dialog_nav.dialogs.borrow() {
            // Un seul axe (dy) -- Confirmer/Annuler empilés verticalement,
            // voir ConfirmDialog dans dialogs.slint.
            DialogSlot::Confirm(d) => {
                if dy != 0 {
                    let next = (self.app.dialog_nav.confirm_nav_index.get() + dy).clamp(0, 1);
                    self.app.dialog_nav.confirm_nav_index.set(next);
                    d.set_selected_index(next);
                }
            }
            // Même pattern -- Réinstaller/Infos empilés verticalement, voir
            // ErrorDialog dans dialogs.slint.
            DialogSlot::Error(d) => {
                if dy != 0 {
                    let next = (self.app.dialog_nav.error_nav_index.get() + dy).clamp(0, 1);
                    self.app.dialog_nav.error_nav_index.set(next);
                    d.set_selected_index(next);
                }
            }
            DialogSlot::Picker(d) => {
                let count = d.get_items().row_count() as i32;
                if count == 0 {
                    return;
                }
                let next = (self.app.dialog_nav.picker_index.get() + dy).clamp(0, count - 1);
                self.app.dialog_nav.picker_index.set(next);
                d.set_selected_index(next);
                d.set_scroll_trigger(!d.get_scroll_trigger());
            }
            // Deux axes INDÉPENDANTS : Haut/Bas défile le texte
            // d'instructions (voir scroll-instructions dans dialogs.slint,
            // seul texte qui peut dépasser la hauteur du dialogue),
            // Gauche/Droite navigue la rangée horizontale de boutons,
            // restreinte aux boutons ACTIVÉS (un bouton désactivé -- pas de
            // dossier de sauvegarde connu, par ex. -- ne doit jamais
            // recevoir la sélection).
            DialogSlot::Info(d) => {
                if dy != 0 {
                    d.invoke_scroll_instructions(dy);
                }
                if dx != 0 {
                    let enabled = info_nav_enabled(d);
                    let candidates: Vec<i32> = (0..9).filter(|&i| enabled[i as usize]).collect();
                    if !candidates.is_empty() {
                        let current = self.app.dialog_nav.info_nav_index.get();
                        let pos = candidates.iter().position(|&i| i == current).unwrap_or(0) as i32;
                        let next_pos = (pos + dx).clamp(0, candidates.len() as i32 - 1);
                        let next = candidates[next_pos as usize];
                        self.app.dialog_nav.info_nav_index.set(next);
                        d.set_selected_index(next);
                    }
                }
            }
            // Un seul axe (dy) -- plus de rangée de raccourcis à naviguer en
            // Gauche/Droite depuis que Settings s'est scindé en un menu
            // séparé (voir open_settings_dialog). Le défilement de liste,
            // dont Picker se passe (ses listes réelles restent courtes), est
            // nécessaire ici : themes.json peut dépasser la centaine
            // d'entrées. `invoke_item_hovered` réutilise le callback du
            // survol souris pour que la sélection clavier/manette déclenche
            // le même comportement (aperçu de thème en direct, ou simple
            // surbrillance pour la langue -- voir open_theme_picker/
            // open_language_picker, chacun câble item-hovered différemment).
            DialogSlot::SearchList(d) => {
                if dy != 0 {
                    let count = d.get_items().row_count() as i32;
                    if count != 0 {
                        let next = (self.app.dialog_nav.picker_index.get() + dy).clamp(0, count - 1);
                        d.set_scroll_trigger(!d.get_scroll_trigger());
                        d.invoke_item_hovered(next);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Cible manette de base (voir ui::gamepad_router) -- reste tout en bas de
/// la pile pour toute la durée de l'appli et aiguille vers la liste fenêtrée
/// ou la grille selon le mode courant. Atteinte uniquement quand aucun
/// dialogue n'est ouvert : le routeur ne dispatche qu'au sommet de la pile,
/// et DialogGamepadTarget s'y empile dès qu'un dialogue s'affiche.
pub(crate) struct AppGamepadTarget {
    pub(crate) app: Rc<AppState>,
    pub(crate) router: Rc<RefCell<crate::ui::gamepad_router::GamepadRouter>>,
}

impl GamepadTarget for AppGamepadTarget {
    fn move_selection(&self, dx: i32, dy: i32) {
        if self.app.window().get_big_mode() {
            self.app.move_grid_selection(dx, dy);
        } else if dx != 0 {
            // Saut de page (voir PAGE_ROWS) -- même règle que Gauche/Droite
            // au clavier, voir on_move_requested dans main().
            self.app.move_windowed_selection(dx * PAGE_ROWS);
        } else {
            self.app.move_windowed_selection(dy);
        }
    }

    fn activate_selection(&self) {
        activate_selection(&self.app, &self.router);
    }

    fn show_info_for_selection(&self) {
        show_info_for_current_selection(&self.app, &self.router);
    }

    fn toggle_fullscreen(&self) {
        self.app.toggle_fullscreen();
    }
}
