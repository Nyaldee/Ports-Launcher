//! Orchestration/état de l'application : `AppState` (voir `state`), la file
//! d'évènements d'arrière-plan qui l'alimente (voir `events`), et les actions
//! déclenchées dessus -- dialogues (`dialogs`), install/lancement
//! (`install_launch`), temps de jeu (`playtime`), synchronisation catalogue/
//! thèmes/auto-MAJ (`sync`), et cible manette de la fenêtre principale (voir
//! `gamepad_target`). `cards` ne construit que le modèle Slint de la grille
//! plein écran, sans état propre.

pub mod cards;
pub mod dialogs;
pub mod events;
pub mod gamepad_target;
pub mod install_launch;
pub mod playtime;
pub mod state;
pub mod sync;
