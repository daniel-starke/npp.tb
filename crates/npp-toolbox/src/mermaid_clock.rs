//! The system clock and the local time zone for Mermaid's gantt charts.

use std::time::{SystemTime, UNIX_EPOCH};

use npp::time_zone::TimeZone;

use crate::mermaid::Clock;

struct SystemClock(TimeZone);

impl Clock for SystemClock {
	fn now(&self) -> f64 {
		match SystemTime::now().duration_since(UNIX_EPOCH) {
			Ok(duration) => duration.as_millis() as f64,
			Err(error) => -(error.duration().as_millis() as f64),
		}
	}

	fn utc_offset(&self, time: f64) -> f64 {
		self.0.utc_offset(time)
	}
}

/// The clock for [`crate::mermaid::DiagramRenderer`].
pub fn system_clock() -> Box<dyn Clock> {
	Box::new(SystemClock(TimeZone::new()))
}
