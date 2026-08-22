//! Test de non-régression textuel sur les fichiers `.slint` : un élément
//! dont `vertical-stretch`/`horizontal-stretch` peut valoir autre chose que
//! `0` (valeur conditionnelle, ou `1`) ne doit pas porter aussi
//! `height`/`width`. En Slint, `height`/`width` est une contrainte dure qui
//! l'emporte toujours sur le stretch ; `preferred-height`/`preferred-width`
//! est la propriété correcte pour une base que le stretch peut agrandir.
//! Symptôme typique : un élément rendu à 0px de haut alors que son stretch
//! aurait dû lui donner de l'espace.
//!
//! `vertical-stretch: 0` littéral combiné à un `height:` fixe reste valide
//! (stretch désactivé, la taille fixe fait foi) et n'est pas signalé.
//!
//! Ne remplace pas une vérification visuelle de l'application compilée :
//! cette analyse cible une seule classe de bug. Ne gère pas les
//! commentaires de bloc `/* */` (aucun dans les .slint du projet), seuls
//! les `//` sont retirés avant l'analyse.

#[cfg(test)]
mod tests {
    const SLINT_FILES: &[(&str, &str)] = &[
        ("app-window.slint", include_str!("../../ui/app-window.slint")),
        ("card-grid.slint", include_str!("../../ui/card-grid.slint")),
        ("theme-colors.slint", include_str!("../../ui/theme-colors.slint")),
        ("dialogs.slint", include_str!("../../ui/dialogs.slint")),
    ];

    /// Les bindings `propriete: valeur;` déclarés directement dans UN
    /// élément (une paire d'accolades), sans descendre dans ses enfants --
    /// chacun ouvre sa propre `Frame` à son propre `{`.
    struct Frame {
        start_line: usize,
        bindings: Vec<(String, String)>,
    }

    fn strip_line_comments(source: &str) -> String {
        source.lines().map(|line| line.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n")
    }

    /// Tokenizer minimal : suit `{`/`}` (profondeur) et `;` (fin de
    /// binding). Suffisant tant qu'aucun .slint du projet ne contient
    /// d'accolade ou de point-virgule à l'intérieur d'une chaîne.
    fn parse_frames(source: &str) -> Vec<Frame> {
        let cleaned = strip_line_comments(source);
        let mut stack: Vec<Frame> = vec![Frame { start_line: 1, bindings: Vec::new() }];
        let mut finished = Vec::new();
        let mut buf = String::new();
        let mut line = 1usize;
        for ch in cleaned.chars() {
            match ch {
                '\n' => {
                    line += 1;
                    buf.push(' ');
                }
                '{' => {
                    buf.clear();
                    stack.push(Frame { start_line: line, bindings: Vec::new() });
                }
                '}' => {
                    buf.clear();
                    if let Some(frame) = stack.pop() {
                        finished.push(frame);
                    }
                }
                ';' => {
                    if let Some((key, value)) = buf.split_once(':') {
                        if let Some(top) = stack.last_mut() {
                            top.bindings.push((key.trim().to_string(), value.trim().to_string()));
                        }
                    }
                    buf.clear();
                }
                other => buf.push(other),
            }
        }
        finished
    }

    fn check_pair(file: &str, frames: &[Frame], stretch_prop: &str, size_prop: &str) -> Vec<String> {
        let mut errors = Vec::new();
        for frame in frames {
            let Some((_, stretch_value)) = frame.bindings.iter().find(|(k, _)| k == stretch_prop) else { continue };
            if stretch_value == "0" {
                continue;
            }
            if frame.bindings.iter().any(|(k, _)| k == size_prop) {
                errors.push(format!(
                    "{file}:~L{} -- `{stretch_prop}: {stretch_value}` combiné à `{size_prop}:` sur le même élément \
                     : utiliser `preferred-{size_prop}:` à la place, `{size_prop}:` étant une contrainte dure qui \
                     écrase le stretch dès qu'il est actif",
                    frame.start_line
                ));
            }
        }
        errors
    }

    #[test]
    fn aucun_element_slint_ne_combine_stretch_actif_et_taille_fixe() {
        let mut all_errors = Vec::new();
        for (name, source) in SLINT_FILES {
            let frames = parse_frames(source);
            all_errors.extend(check_pair(name, &frames, "vertical-stretch", "height"));
            all_errors.extend(check_pair(name, &frames, "horizontal-stretch", "width"));
        }
        assert!(all_errors.is_empty(), "\n{}", all_errors.join("\n"));
    }
}
