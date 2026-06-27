pub const ELEVATION_HANDOFF_ARG: &str = "--elevation-handoff";

pub fn is_elevation_handoff(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == ELEVATION_HANDOFF_ARG || arg.starts_with(&format!("{ELEVATION_HANDOFF_ARG}="))
    })
}

pub fn elevation_command_line() -> String {
    elevation_command_line_with_autostart(crate::startup::is_autostart_launch())
}

pub fn elevation_command_line_with_autostart(autostart: bool) -> String {
    let mut args = vec![ELEVATION_HANDOFF_ARG.to_string()];
    if autostart {
        args.push(crate::startup::AUTOSTART_ARG.to_string());
    }
    args.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_elevation_handoff_arg() {
        assert!(is_elevation_handoff(&[
            "omnafk.exe".into(),
            ELEVATION_HANDOFF_ARG.into()
        ]));
        assert!(!is_elevation_handoff(&[
            "omnafk.exe".into(),
            "--autostart".into()
        ]));
    }

    #[test]
    fn elevation_command_line_preserves_autostart() {
        assert_eq!(
            elevation_command_line_with_autostart(true),
            "--elevation-handoff --autostart"
        );
        assert_eq!(
            elevation_command_line_with_autostart(false),
            "--elevation-handoff"
        );
    }
}
