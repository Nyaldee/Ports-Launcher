
const SLINT_FILES: &[(&str, &str)] = &[
    ("app-window.slint", include_str!("../../ui/app-window.slint")),
    ("card-grid.slint", include_str!("../../ui/card-grid.slint")),
    ("shared.slint", include_str!("../../ui/shared.slint")),
    ("tray.slint", include_str!("../../ui/tray.slint")),
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
    let mut stack = vec![Frame { start_line: 1, bindings: Vec::new() }];
    let mut finished = Vec::new();
    let mut buf = String::new();
    let mut line = 1usize;
    for ch in strip_line_comments(source).chars() {
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
                finished.extend(stack.pop());
            }
            ';' => {
                if let (Some((key, value)), Some(top)) = (buf.split_once(':'), stack.last_mut()) {
                    top.bindings.push((key.trim().to_string(), value.trim().to_string()));
                }
                buf.clear();
            }
            other => buf.push(other),
        }
    }
    finished
}

fn check_pair(file: &str, frames: &[Frame], stretch_prop: &str, size_prop: &str) -> Vec<String> {
    frames
        .iter()
        .filter_map(|frame| {
            let (_, stretch_value) = frame.bindings.iter().find(|(k, _)| k == stretch_prop)?;
            (stretch_value != "0" && frame.bindings.iter().any(|(k, _)| k == size_prop)).then(|| {
                format!("{file}:~L{} -- `{stretch_prop}: {stretch_value}` avec `{size_prop}:` ; utiliser `preferred-{size_prop}:`", frame.start_line)
            })
        })
        .collect()
}

#[test]
fn aucun_element_slint_ne_combine_stretch_actif_et_taille_fixe() {
    let errors: Vec<String> = SLINT_FILES
        .iter()
        .flat_map(|(name, source)| {
            let frames = parse_frames(source);
            let mut errors = check_pair(name, &frames, "vertical-stretch", "height");
            errors.extend(check_pair(name, &frames, "horizontal-stretch", "width"));
            errors
        })
        .collect();
    assert!(errors.is_empty(), "\n{}", errors.join("\n"));
}
