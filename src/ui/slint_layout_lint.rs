
#[cfg(test)]
mod tests {
    const SLINT_FILES: &[(&str, &str)] = &[
        ("app-window.slint", include_str!("../../ui/app-window.slint")),
        ("card-grid.slint", include_str!("../../ui/card-grid.slint")),
        ("shared.slint", include_str!("../../ui/shared.slint")),
        ("dialogs/chrome.slint", include_str!("../../ui/dialogs/chrome.slint")),
        ("dialogs/simple.slint", include_str!("../../ui/dialogs/simple.slint")),
        ("dialogs/picker.slint", include_str!("../../ui/dialogs/picker.slint")),
        ("dialogs/info.slint", include_str!("../../ui/dialogs/info.slint")),
    ];

    struct Frame {
        start_line: usize,
        bindings: Vec<(String, String)>,
    }

    fn strip_line_comments(source: &str) -> String {
        source.lines().map(|line| line.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n")
    }

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
