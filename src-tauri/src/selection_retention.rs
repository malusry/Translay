use std::time::Duration;

pub(crate) const FADE_DURATION: Duration = Duration::from_millis(120);
const APPROACH_GRACE: Duration = Duration::from_millis(500);
const LEAVE_GRACE: Duration = Duration::from_millis(350);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Proximity {
    Away,
    Near,
    Hover,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Keep,
    Restore,
    Fade,
    Hide,
}

pub(crate) struct Retention {
    deadline: Duration,
    approach_used: bool,
    hovered: bool,
    fading_since: Option<Duration>,
}

impl Retention {
    pub(crate) fn new(duration: Duration) -> Self {
        Self {
            deadline: duration,
            approach_used: false,
            hovered: false,
            fading_since: None,
        }
    }

    pub(crate) fn tick(&mut self, now: Duration, proximity: Proximity) -> Action {
        if proximity == Proximity::Hover {
            self.hovered = true;
            return if self.fading_since.take().is_some() {
                Action::Restore
            } else {
                Action::Keep
            };
        }
        if self.hovered {
            self.hovered = false;
            self.deadline = self.deadline.max(now + LEAVE_GRACE);
        }
        if now < self.deadline {
            return Action::Keep;
        }
        if proximity == Proximity::Near && !self.approach_used && self.fading_since.is_none() {
            self.approach_used = true;
            self.deadline = now + APPROACH_GRACE;
            return Action::Keep;
        }
        match self.fading_since {
            Some(start) if now.saturating_sub(start) >= FADE_DURATION => Action::Hide,
            Some(_) => Action::Keep,
            None => {
                self.fading_since = Some(now);
                Action::Fade
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }
    #[test]
    fn normal_expiry_fades_then_hides() {
        let mut p = Retention::new(ms(5000));
        assert_eq!(p.tick(ms(4999), Proximity::Away), Action::Keep);
        assert_eq!(p.tick(ms(5000), Proximity::Away), Action::Fade);
        assert_eq!(p.tick(ms(5119), Proximity::Away), Action::Keep);
        assert_eq!(p.tick(ms(5120), Proximity::Away), Action::Hide);
    }
    #[test]
    fn stationary_nearby_pointer_cannot_extend_forever() {
        let mut p = Retention::new(ms(5000));
        assert_eq!(p.tick(ms(5000), Proximity::Near), Action::Keep);
        assert_eq!(p.tick(ms(5500), Proximity::Near), Action::Fade);
        assert_eq!(p.tick(ms(5620), Proximity::Near), Action::Hide);
    }
    #[test]
    fn hovering_and_returning_during_leave_grace_preserve_target() {
        let mut p = Retention::new(ms(5000));
        assert_eq!(p.tick(ms(6000), Proximity::Hover), Action::Keep);
        assert_eq!(p.tick(ms(7000), Proximity::Away), Action::Keep);
        assert_eq!(p.tick(ms(7300), Proximity::Hover), Action::Keep);
        assert_eq!(p.tick(ms(8000), Proximity::Away), Action::Keep);
        assert_eq!(p.tick(ms(8350), Proximity::Away), Action::Fade);
    }
    #[test]
    fn entering_during_fade_restores_and_starts_fresh_leave_grace() {
        let mut p = Retention::new(ms(5000));
        assert_eq!(p.tick(ms(5000), Proximity::Away), Action::Fade);
        assert_eq!(p.tick(ms(5070), Proximity::Hover), Action::Restore);
        assert_eq!(p.tick(ms(5100), Proximity::Away), Action::Keep);
        assert_eq!(p.tick(ms(5450), Proximity::Away), Action::Fade);
    }
}
