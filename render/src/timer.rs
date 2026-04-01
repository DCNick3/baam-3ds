pub struct PeriodicSkippingTimer {
    period: u32,
    next_tick: u32,
}
impl PeriodicSkippingTimer {
    pub fn new(period: u32) -> Self {
        Self {
            period,
            next_tick: 0,
        }
    }

    pub fn reset(&mut self, tick: u32) {
        self.next_tick = tick + self.period;
    }

    pub fn update(&mut self, tick: u32) -> bool {
        if tick < self.next_tick {
            return false;
        }

        let periods_behind = (tick - self.next_tick) / self.period + 1;
        self.next_tick += self.period * periods_behind;

        true
    }
}

pub struct BackoffTimer {
    next_tick: u32,
}

impl BackoffTimer {
    pub fn new() -> Self {
        Self { next_tick: 0 }
    }

    pub fn ready(&self, tick: u32) -> bool {
        tick >= self.next_tick
    }

    pub fn delay(&mut self, delay: u32) {
        self.next_tick += delay;
    }

    pub fn reset(&mut self, tick: u32) {
        self.next_tick = tick;
    }
}
