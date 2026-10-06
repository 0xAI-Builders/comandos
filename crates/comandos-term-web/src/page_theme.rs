//! Literal page palettes from the legacy terminal, including CSS chrome colors.
pub const THEMES: &[(&str, &str)] = &[
    (
        "noche",
        r##"{"background":"#0A0D13","foreground":"#EAF0FB","cursor":"#FFAE1A","cursorAccent":"#0A0D13","selectionBackground":"#2E3852","panel":"#121722","panel2":"#161C29","line":"#222A3A","line2":"#2E3852","dim":"#9AA6BF","faint":"#5E6980","brand":"#8B7CFF"}"##,
    ),
    (
        "dia",
        r##"{"background":"#BCC7D4","foreground":"#17202B","cursor":"#1D55C7","cursorAccent":"#FFFFFF","selectionBackground":"#98A6B6","panel":"#F1F4F7","panel2":"#DDE4EB","line":"#98A6B6","line2":"#687A90","dim":"#3E4D61","faint":"#5E7087","brand":"#1D55C7"}"##,
    ),
    (
        "calido",
        r##"{"background":"#161009","foreground":"#F2E5D0","cursor":"#FFB454","cursorAccent":"#161009","selectionBackground":"#36291A","panel":"#1F1811","panel2":"#261D14","line":"#36291A","line2":"#4A3823","dim":"#BCA98C","faint":"#8A7A5F","brand":"#E0A458"}"##,
    ),
    (
        "termius",
        r##"{"background":"#0E1620","foreground":"#D6E0EA","cursor":"#FFAE1A","cursorAccent":"#0E1620","selectionBackground":"#25384F","panel":"#141E2A","panel2":"#182432","line":"#1C2733","line2":"#25384F","dim":"#8FA0B4","faint":"#5C6B80","brand":"#4CE07A"}"##,
    ),
    (
        "bruno",
        r##"{"background":"#1A1A1A","foreground":"#CCCCCC","cursor":"#E4AE49","cursorAccent":"#1A1A1A","selectionBackground":"#444444","panel":"#222224","panel2":"#26292B","line":"#333333","line2":"#444444","dim":"#AAAAAA","faint":"#999999","brand":"#E4AE49","black":"#888888","red":"#DA462F","green":"#73E89A","yellow":"#FAD075","blue":"#8BC2F9","magenta":"#D691ED","cyan":"#7DDFF2","white":"#CCCCCC","brightBlack":"#666666","brightRed":"#F38172","brightGreen":"#73E89A","brightYellow":"#FAD075","brightBlue":"#8BC2F9","brightMagenta":"#D691ED","brightCyan":"#7DDFF2","brightWhite":"#FFFFFF"}"##,
    ),
    (
        "superglass",
        r##"{"background":"#080B19","foreground":"#F2F5FF","cursor":"#72E6FF","cursorAccent":"#061119","selectionBackground":"#34416C","panel":"#171B36","panel2":"#202749","line":"#34416C","line2":"#53679A","dim":"#B7C3E8","faint":"#8291BD","brand":"#72E6FF"}"##,
    ),
    (
        "neon",
        r##"{"background":"#07080D","foreground":"#F7F7FC","cursor":"#28E7F2","cursorAccent":"#041214","selectionBackground":"#33384B","panel":"#11131B","panel2":"#181B27","line":"#33384B","line2":"#596079","dim":"#B7B8CA","faint":"#85879F","brand":"#28E7F2"}"##,
    ),
    (
        "contraste",
        r##"{"background":"#000000","foreground":"#FFFFFF","cursor":"#59B0FF","cursorAccent":"#000000","selectionBackground":"#8A8A8A","panel":"#0B0B0B","panel2":"#171717","line":"#8A8A8A","line2":"#FFFFFF","dim":"#E0E0E0","faint":"#B8B8B8","brand":"#59B0FF"}"##,
    ),
    (
        "ubuntu",
        r##"{"background":"#300A24","foreground":"#FFFFFF","cursor":"#FFFFFF","cursorAccent":"#300A24","selectionBackground":"#7A4069","panel":"#3C1531","panel2":"#48193C","line":"#5C2A4F","line2":"#7A4069","dim":"#E8DCE4","faint":"#BBA5B5","brand":"#E95420","black":"#2E3436","red":"#CC0000","green":"#4E9A06","yellow":"#C4A000","blue":"#3465A4","magenta":"#75507B","cyan":"#06989A","white":"#D3D7CF","brightBlack":"#555753","brightRed":"#EF2929","brightGreen":"#8AE234","brightYellow":"#FCE94F","brightBlue":"#729FCF","brightMagenta":"#AD7FA8","brightCyan":"#34E2E2","brightWhite":"#EEEEEC"}"##,
    ),
];
pub fn theme(name: &str) -> Option<&'static str> {
    THEMES.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_color_is_literal_legacy_data() {
        let original = include_str!("../../../dash/term.html");
        let block = original
            .split("const THEMES = {")
            .nth(1)
            .unwrap()
            .split("\n  };")
            .next()
            .unwrap();
        let mut count = 0;
        for line in block.lines().filter(|line| line.contains('{')) {
            let (name, fields) = line.trim().split_once(':').unwrap();
            let fields = fields
                .trim()
                .trim_start_matches('{')
                .trim_end_matches(',')
                .trim_end_matches('}');
            let mut expected = serde_json::Map::new();
            for pair in fields.split(',') {
                let (key, val) = pair.split_once(':').unwrap();
                expected.insert(key.into(), val.trim_matches('\'').into());
            }
            let actual: serde_json::Value =
                serde_json::from_str(super::theme(name).unwrap()).unwrap();
            assert_eq!(actual, serde_json::Value::Object(expected), "{name}");
            count += 1;
        }
        assert_eq!(count, super::THEMES.len());
    }
}
