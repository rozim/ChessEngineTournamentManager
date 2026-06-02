//! Per-engine standings and a relative Elo estimate.

/// Running tally of results for one engine.
#[derive(Default, Clone)]
pub struct Standing {
    pub name: String,
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
    /// Points scored in each mini-match this engine played (one value per
    /// pair, each in 0.0..=2.0). The pentanomial unit for the Elo confidence
    /// interval — it keeps the two color-swapped games of a pair together.
    pub pair_points: Vec<f64>,
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

    /// A 95% confidence interval half-width (in Elo points) for the relative
    /// performance rating, estimated from the per-mini-match pair scores.
    ///
    /// Using the pair (two color-swapped games) as the sampling unit is the
    /// pentanomial model: it respects the correlation between a pair's games,
    /// unlike treating individual games as independent. Returns `None` when
    /// there are fewer than two pairs or the score sits at 0% / 100% (where
    /// the Elo transform is undefined).
    pub fn elo_ci95(&self) -> Option<f64> {
        let n = self.pair_points.len();
        if n < 2 {
            return None;
        }
        let n_f = n as f64;
        let mean = self.pair_points.iter().sum::<f64>() / n_f; // mean of 0..=2
        // Unbiased sample variance of the per-pair score.
        let var = self
            .pair_points
            .iter()
            .map(|x| (x - mean).powi(2))
            .sum::<f64>()
            / (n_f - 1.0);
        let se_mean = (var / n_f).sqrt(); // standard error of the mean pair score
        let p = mean / 2.0; // score fraction in 0..=1
        if !(f64::EPSILON..=1.0 - f64::EPSILON).contains(&p) {
            return None;
        }
        let se_p = se_mean / 2.0;
        // Propagate through Elo(p) = -400 log10(1/p - 1); dElo/dp = (400/ln10)/(p(1-p)).
        let d_elo_dp = (400.0 / std::f64::consts::LN_10) / (p * (1.0 - p));
        Some(1.96 * d_elo_dp * se_p)
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

    #[test]
    fn elo_ci_needs_at_least_two_pairs() {
        let mut s = Standing::new("e");
        s.pair_points = vec![1.0];
        assert!(s.elo_ci95().is_none());
    }

    #[test]
    fn elo_ci_positive_with_spread() {
        let mut s = Standing::new("e");
        s.pair_points = vec![2.0, 1.0, 1.0, 0.0, 1.5, 0.5];
        let ci = s.elo_ci95().expect("ci");
        assert!(ci > 0.0 && ci.is_finite());
    }

    #[test]
    fn elo_ci_none_at_extreme_score() {
        let mut s = Standing::new("e");
        s.pair_points = vec![2.0, 2.0, 2.0];
        assert!(s.elo_ci95().is_none());
    }

    #[test]
    fn elo_ci_zero_when_no_variance() {
        let mut s = Standing::new("e");
        s.pair_points = vec![1.0, 1.0, 1.0, 1.0];
        assert_eq!(s.elo_ci95(), Some(0.0));
    }
}
