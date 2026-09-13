//! Version du build en cours d'exécution -- comparée au tag de la dernière
//! release GitHub du launcher lui-même (voir main.rs, vérification en tâche
//! de fond) pour savoir si le bouton GitHub doit se transformer en bouton
//! "Update".
//!
//! Calculée automatiquement par `build.rs` à partir de la date du jour (UTC)
//! au moment de la compilation, au même format que les tags de release
//! (`MM/DD/YY`, ex. "08/19/26") -- rien à saisir à la main ici.
//!
//! Chaque publication portant un tag garanti différent, la comparaison de
//! tag seule suffit pour le launcher ; le repli par date d'`update_decision`
//! (voir github_api.rs) ne concerne que les ports tiers, dont les
//! mainteneurs peuvent recycler un tag.
//!
//! Contrainte de publication : compiler ET publier le tag GitHub le MÊME
//! jour calendaire UTC -- sinon la date embarquée dans le build et le tag
//! publié divergent d'un jour, et ce build ne se reconnaît plus jamais
//! comme à jour.

pub const APP_VERSION: &str = env!("APP_BUILD_DATE");

/// `installed_at` neutre à passer à `github_api::check_update_available`
/// pour le check du launcher lui-même : seule la comparaison de tag doit
/// compter ici. Une date dans le futur lointain rend `latest_date >
/// installed_at` toujours faux, donc le repli par date ne peut jamais
/// déclencher à lui seul un faux "Update".
pub const NEUTRAL_INSTALLED_AT: &str = "9999-12-31T23:59:59Z";
