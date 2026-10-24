use super::event::Frame;

const APP_PREFIX: &str = concat!(env!("CARGO_CRATE_NAME"), "::");

/// Parses the text of `std::backtrace::Backtrace`:
///
/// ```text
///    3: pills_server_test::handlers::persons::get_person
///              at ./src/handlers/persons.rs:42:9
/// ```
///
/// Frames from the panic machinery itself are dropped.
pub fn parse(backtrace: &str) -> Vec<Frame> {
    let mut frames: Vec<Frame> = Vec::new();
    for line in backtrace.lines() {
        let line = line.trim();
        if let Some(location) = line.strip_prefix("at ") {
            if let Some(frame) = frames.last_mut() {
                let (file, number) = split_location(location);
                frame.file = Some(file.to_string());
                frame.line = number;
            }
        } else if let Some((index, function)) = line.split_once(": ")
            && !index.is_empty()
            && index.bytes().all(|b| b.is_ascii_digit())
        {
            frames.push(Frame {
                function: function.to_string(),
                file: None,
                line: None,
                in_app: function.starts_with(APP_PREFIX)
                    || function.starts_with(&format!("<{APP_PREFIX}")),
            });
        }
    }

    let panic_frames = frames
        .iter()
        .rposition(|frame| is_panic_machinery(&frame.function))
        .map_or(0, |last| last + 1);
    frames.split_off(panic_frames.min(frames.len()))
}

/// Frames between the panic and the hook. `std::panicking::try` and
/// `catch_unwind` are not among them: they sit below the panicking code.
fn is_panic_machinery(function: &str) -> bool {
    const PREFIXES: [&str; 9] = [
        "std::backtrace",
        "std::sys::backtrace::__rust_end_short_backtrace",
        "std::panicking::begin_panic",
        "std::panicking::rust_panic",
        "std::panicking::panic_with_hook",
        "core::panicking",
        "rust_begin_unwind",
        "__rustc::rust_begin_unwind",
        "<alloc::boxed::Box<F,A> as core::ops::function::Fn<Args>>::call",
    ];
    PREFIXES.iter().any(|prefix| function.starts_with(prefix))
        || function.contains("error_reporting::panic_hook")
}

/// `path:line:column` or `path:line`; the path may contain `:` (Windows drives).
fn split_location(location: &str) -> (&str, Option<u32>) {
    let Some((rest, last)) = location.rsplit_once(':') else {
        return (location, None);
    };
    let Ok(last) = last.parse::<u32>() else {
        return (location, None);
    };
    if let Some((file, line)) = rest.rsplit_once(':')
        && let Ok(line) = line.parse::<u32>()
    {
        return (file, Some(line));
    }
    (rest, Some(last))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "   0: std::backtrace_rs::backtrace::trace_unsynchronized
             at /rustc/abc/library/std/src/../../backtrace/src/backtrace/mod.rs:66:5
   1: std::backtrace::Backtrace::create
             at /rustc/abc/library/std/src/backtrace.rs:331:13
   2: pills_server_test::error_reporting::panic_hook
             at ./src/error_reporting/mod.rs:90:25
   3: std::panicking::rust_panic_with_hook
             at /rustc/abc/library/std/src/panicking.rs:841:13
   4: core::panicking::panic_fmt
             at /rustc/abc/library/core/src/panicking.rs:75:14
   5: pills_server_test::handlers::persons::get_person
             at C:\\work\\src\\handlers\\persons.rs:42:9
   6: tokio::runtime::task::harness::poll_future
   7: <pills_server_test::Thing as core::future::Future>::poll
             at ./src/lib.rs:10:1";

    #[test]
    fn keeps_frames_from_the_panicking_code_down() {
        let frames = parse(SAMPLE);

        assert_eq!(frames.len(), 3);
        assert_eq!(
            frames[0].function,
            "pills_server_test::handlers::persons::get_person"
        );
        assert_eq!(
            frames[0].file.as_deref(),
            Some("C:\\work\\src\\handlers\\persons.rs")
        );
        assert_eq!(frames[0].line, Some(42));
        assert!(frames[0].in_app);
        assert_eq!(
            frames[1].function,
            "tokio::runtime::task::harness::poll_future"
        );
        assert_eq!(frames[1].file, None);
        assert!(!frames[1].in_app);
        assert!(frames[2].in_app);
    }

    #[test]
    fn runtime_frames_below_the_app_are_kept() {
        let frames = parse(
            "   0: std::panicking::rust_panic_with_hook
   1: std::sys::backtrace::__rust_end_short_backtrace<closure>
   2: core::panicking::panic_fmt
   3: pills_server_test::main
             at .\\src\\main.rs:20:37
   4: std::sys::backtrace::__rust_begin_short_backtrace<fn()>
   5: std::panicking::catch_unwind
   6: invoke_main
             at D:\\a\\s\\exe_common.inl:78",
        );

        let functions: Vec<&str> = frames.iter().map(|f| f.function.as_str()).collect();
        assert_eq!(
            functions,
            [
                "pills_server_test::main",
                "std::sys::backtrace::__rust_begin_short_backtrace<fn()>",
                "std::panicking::catch_unwind",
                "invoke_main"
            ]
        );
        assert_eq!(frames[0].line, Some(20));
        assert_eq!(frames[3].file.as_deref(), Some("D:\\a\\s\\exe_common.inl"));
        assert_eq!(frames[3].line, Some(78));
    }

    #[test]
    fn locations_with_and_without_column() {
        assert_eq!(split_location("C:\\x.rs:42:9"), ("C:\\x.rs", Some(42)));
        assert_eq!(split_location("C:\\x.inl:288"), ("C:\\x.inl", Some(288)));
        assert_eq!(
            split_location("./src/lib.rs:7:1"),
            ("./src/lib.rs", Some(7))
        );
        assert_eq!(split_location("unknown"), ("unknown", None));
    }

    #[test]
    fn unparsed_text_gives_no_frames() {
        assert!(parse("disabled backtrace").is_empty());
    }
}
