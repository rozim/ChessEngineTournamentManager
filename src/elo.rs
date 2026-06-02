//! Per-engine standings and a relative Elo estimate.

/// Running tally of results for one engine.
#[derive(Default, Clone)]
pub struct Standing {
    pub name: String,
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
}

impl Standing {
    pub fn new(name: &str) -> Standing {
        Standing {
            name: name.to_string(),
            ..Default::default()
        }
    }

    pub fn games(&self) -> u32 {
        self.wins + self.draws + self.losses
    }

    /// win = 1, draw = 0.5, loss = 0.
    pub fn points(&self) -> f64 {
        self.wins as f64 + 0.5 * self.draws as f64
    }

    /// Performance rating relative to the field (0 = average), derived from
    /// the score fraction with the logistic Elo formula. Clamped so a perfect
    /// or zero score yields a finite, capped value.
    pub fn relative_elo(&self) -> f64 {
        let games = self.games();
        if games == 0 {
            return 0.0;
        }
        let score = self.points() / games as f64;
        let eps = 1.0 / (2.0 * games as f64 + 2.0);
        let clamped = score.clamp(eps, 1.0 - eps);
        let elo = -400.0 * (1.0 / clamped - 1.0).log10();
        // Normalize -0.0 to 0.0 for tidy display.
        if elo == 0.0 {
            0.0
        } else {
            elo
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_and_game_count() {
        let mut s = Standing::new("e");
        s.wins = 3;
        s.draws = 2;
        s.losses = 1;
        assert_eq!(s.games(), 6);
        assert_eq!(s.points(), 4.0);
    }

    #[test]
    fn even_score_is_zero_elo() {
        let mut s = Standing::new("e");
        s.wins = 5;
        s.losses = 5;
        assert!(s.relative_elo().abs() < 1e-6);
    }

    #[test]
    fn winning_record_is_positive_elo() {
        let mut s = Standing::new("e");
        s.wins = 8;
        s.losses = 2;
        assert!(s.relative_elo() > 0.0);
    }

    #[test]
    fn no_games_is_zero_elo() {
        assert_eq!(Standing::new("e").relative_elo(), 0.0);
    }
}
