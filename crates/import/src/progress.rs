//! Progress reporting for long-running import stages.

use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

/// A set of progress bars; hidden in tests and non-interactive runs.
#[derive(Debug, Clone)]
pub struct Progress {
    multi: MultiProgress,
}

impl Progress {
    /// Bars drawn to stderr (indicatif hides them automatically when stderr is
    /// not a terminal).
    #[must_use]
    pub fn stderr() -> Self {
        Self {
            multi: MultiProgress::new(),
        }
    }

    /// No output at all.
    #[must_use]
    pub fn hidden() -> Self {
        Self {
            multi: MultiProgress::with_draw_target(ProgressDrawTarget::hidden()),
        }
    }

    /// A bar with a known total.
    #[must_use]
    pub fn bar(&self, label: &str, total: u64) -> ProgressBar {
        let bar = self.multi.add(ProgressBar::new(total));
        bar.set_style(
            ProgressStyle::with_template(
                "{prefix:>22.bold} [{bar:30.cyan/blue}] {human_pos:>11}/{human_len:<11} {per_sec:>12} eta {eta}",
            )
            .expect("valid template")
            .progress_chars("=> "),
        );
        bar.set_prefix(label.to_string());
        bar.enable_steady_tick(Duration::from_millis(250));
        bar
    }

    /// A spinner for steps without a meaningful total.
    #[must_use]
    pub fn spinner(&self, label: &str) -> ProgressBar {
        let bar = self.multi.add(ProgressBar::new_spinner());
        bar.set_style(
            ProgressStyle::with_template("{prefix:>22.bold} {spinner} {msg} {elapsed}")
                .expect("valid template"),
        );
        bar.set_prefix(label.to_string());
        bar.enable_steady_tick(Duration::from_millis(250));
        bar
    }

    /// Print a line above the bars.
    pub fn println(&self, msg: impl AsRef<str>) {
        let _ = self.multi.println(msg);
    }
}
