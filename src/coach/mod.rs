//! Rule-based driving coach (no AI): lap comparison, trail-braking rating, feedback texts and
//! session summaries. Ported from the former Python tools in `tools/`.

pub mod advice;
pub mod compare;
pub mod feedback;
pub mod fuelsave;
pub mod incident;
pub mod lap;
pub mod library;
pub mod report;
pub mod shifts;
pub mod trailbrake;
pub mod worker;
