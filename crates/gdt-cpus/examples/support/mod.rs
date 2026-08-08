use gdt_cpus::{AppliedPriority, Grant, ThreadPriority};
#[cfg(test)]
use gdt_cpus::{FallbackReason, Mechanism, MechanismPolicy};

pub fn priority_detail(
    _requested: ThreadPriority,
    result: &gdt_cpus::Result<AppliedPriority>,
) -> String {
    match result {
        Ok(applied) => {
            let mut text = String::new();
            if applied.requested() != applied.effective() {
                text.push_str("-> ");
                text.push_str(&applied.effective().to_string());
                text.push(' ');
            }
            let mut tags = Vec::new();
            if applied.grant() != Grant::Direct {
                tags.push(applied.grant().to_string());
            }
            if let Some(reason) = applied.reason() {
                tags.push(format!("{reason:?}"));
            }
            if !tags.is_empty() {
                text.push('[');
                text.push_str(&tags.join(", "));
                text.push_str("] ");
            }
            if let Some(broker_error) = applied.broker_error() {
                text.push('(');
                text.push_str(&broker_error.to_string());
                text.push_str(") ");
            }
            text.push_str(&applied.mechanism().to_string());
            text
        }
        Err(e) => format!("FAILED: {e}"),
    }
}

pub fn priority_bracket(
    requested: ThreadPriority,
    result: &gdt_cpus::Result<AppliedPriority>,
) -> String {
    priority_detail(requested, result)
}

#[allow(dead_code)]
pub struct PriorityTally {
    requested: ThreadPriority,
    pub total: usize,
    pub clean: usize,
    pub clamped: usize,
    pub fell_back: usize,
    pub failed: usize,
    sample: Option<gdt_cpus::Result<AppliedPriority>>,
}

#[allow(dead_code)]
impl PriorityTally {
    pub fn new(requested: ThreadPriority) -> Self {
        Self {
            requested,
            total: 0,
            clean: 0,
            clamped: 0,
            fell_back: 0,
            failed: 0,
            sample: None,
        }
    }

    pub fn record(&mut self, result: gdt_cpus::Result<AppliedPriority>) {
        if self.sample.is_none() {
            self.sample = Some(result.clone());
        }
        self.total += 1;
        match result {
            Ok(applied) if applied.requested() != applied.effective() => self.fell_back += 1,
            Ok(applied) if applied.reason().is_some() => self.clamped += 1,
            Ok(_) => self.clean += 1,
            Err(_) => self.failed += 1,
        }
    }

    pub fn render(&self) -> String {
        let Some(sample) = &self.sample else {
            return format!("requested {:?} - not measured", self.requested);
        };
        if self.total > 1 {
            format!(
                "{:?} {}; clean {}/{}, clamped {}/{}, fallback {}/{}, failed {}/{}",
                self.requested,
                priority_bracket(self.requested, sample),
                self.clean,
                self.total,
                self.clamped,
                self.total,
                self.fell_back,
                self.total,
                self.failed,
                self.total
            )
        } else {
            format!(
                "{:?} {}",
                self.requested,
                priority_bracket(self.requested, sample)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn applied(
        requested: ThreadPriority,
        effective: ThreadPriority,
        grant: Grant,
        reason: Option<FallbackReason>,
        mechanism: Mechanism,
    ) -> AppliedPriority {
        AppliedPriority::from_parts(requested, effective, grant, reason, mechanism)
    }

    #[test]
    fn priority_detail_omits_duplicated_clean_priority_name() {
        let applied = applied(
            ThreadPriority::Highest,
            ThreadPriority::Highest,
            Grant::Direct,
            None,
            Mechanism {
                policy: MechanismPolicy::Nice,
                value: -10,
            },
        );
        assert_eq!(
            priority_detail(ThreadPriority::Highest, &Ok(applied)),
            "nice -10"
        );
    }

    #[test]
    fn priority_detail_prints_mechanism_before_broker_and_clamp() {
        let applied = applied(
            ThreadPriority::TimeCritical,
            ThreadPriority::TimeCritical,
            Grant::Brokered,
            Some(FallbackReason::Clamped),
            Mechanism {
                policy: MechanismPolicy::Nice,
                value: -15,
            },
        );
        assert_eq!(
            priority_detail(ThreadPriority::TimeCritical, &Ok(applied)),
            "[Brokered, Clamped] nice -15"
        );
    }

    #[test]
    fn priority_detail_prints_clean_brokered_grants_before_mechanism() {
        let applied = applied(
            ThreadPriority::Highest,
            ThreadPriority::Highest,
            Grant::Brokered,
            None,
            Mechanism {
                policy: MechanismPolicy::Nice,
                value: -10,
            },
        );
        assert_eq!(
            priority_detail(ThreadPriority::Highest, &Ok(applied)),
            "[Brokered] nice -10"
        );
    }

    #[test]
    fn priority_detail_shows_fallback_effective_level() {
        let applied = applied(
            ThreadPriority::Highest,
            ThreadPriority::Normal,
            Grant::Direct,
            Some(FallbackReason::NoBroker),
            Mechanism {
                policy: MechanismPolicy::Nice,
                value: 0,
            },
        );
        assert_eq!(
            priority_detail(ThreadPriority::Highest, &Ok(applied)),
            "-> Normal [NoBroker] nice 0"
        );
    }

    #[test]
    fn priority_bracket_keeps_padding_outside_the_bracket() {
        let applied = applied(
            ThreadPriority::TimeCritical,
            ThreadPriority::TimeCritical,
            Grant::Brokered,
            Some(FallbackReason::Clamped),
            Mechanism {
                policy: MechanismPolicy::Nice,
                value: -15,
            },
        );
        assert_eq!(
            priority_bracket(ThreadPriority::TimeCritical, &Ok(applied)),
            "[Brokered, Clamped] nice -15"
        );
    }

    #[test]
    fn priority_tally_distinguishes_fallback_from_clean_grants() {
        let mut tally = PriorityTally::new(ThreadPriority::AboveNormal);
        tally.record(Ok(applied(
            ThreadPriority::AboveNormal,
            ThreadPriority::Normal,
            Grant::Direct,
            Some(FallbackReason::NoBroker),
            Mechanism {
                policy: MechanismPolicy::Nice,
                value: 0,
            },
        )));
        tally.record(Ok(applied(
            ThreadPriority::AboveNormal,
            ThreadPriority::AboveNormal,
            Grant::Direct,
            None,
            Mechanism {
                policy: MechanismPolicy::Nice,
                value: -5,
            },
        )));

        assert_eq!(tally.total, 2);
        assert_eq!(tally.clean, 1);
        assert_eq!(tally.fell_back, 1);
        assert_eq!(
            tally.render(),
            "AboveNormal -> Normal [NoBroker] nice 0; clean 1/2, clamped 0/2, fallback 1/2, failed 0/2"
        );
    }
}
