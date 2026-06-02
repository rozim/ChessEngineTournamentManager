# Standings statistics: relative Elo and its 95% confidence interval

This document explains the two numbers reported per engine in the final
standings table: the **relative Elo** and its **95% confidence interval (CI)**.

## Inputs

For each engine, after all games:

- `points = wins + 0.5 * draws` (a loss scores 0).
- `games  = wins + draws + losses`.
- `p = points / games` — the **score fraction** (0..1), the engine's average
  result against the whole field.

## Relative Elo (point estimate)

The relative Elo is a *performance rating against the field*, from the standard
logistic (Elo) relation between score fraction and rating difference:

```
Elo = -400 * log10(1/p - 1)
```

- `p = 0.5` → `Elo = 0` (even with the field).
- `p > 0.5` → positive; `p < 0.5` → negative.

To keep the point estimate finite at a clean sweep, `p` is clamped away from
0 and 1 by `eps = 1 / (2*games + 2)` before the transform.

This is *relative* to the field that actually played — it is not an absolute,
cross-tournament rating.

## 95% confidence interval (pentanomial)

The score fraction `p` is an estimate from a finite number of games, so it has
sampling uncertainty. The CI quantifies it.

### Why the *pair* is the sampling unit (pentanomial, not trinomial)

Games are not independent: each **mini-match** is two games from the *same*
opening with the colors swapped, so a pair's two results are correlated.
Treating individual games as independent (the "trinomial" model) misestimates
the variance. Instead we use the **mini-match pair as the sampling unit** — the
"pentanomial" model — where each pair contributes a single score

```
s ∈ {0, 0.5, 1, 1.5, 2}   (the engine's points over the pair's two games)
```

This is the modern standard in engine testing (e.g. Fishtest) precisely
because openings are played in color-swapped pairs.

### The computation

Let the engine have `n` mini-match pairs with scores `s_1 … s_n`
(note `Σ s_i = points` and `n = games / 2`):

1. Mean pair score: `m = (Σ s_i) / n`.
2. Unbiased sample variance: `v = Σ (s_i - m)^2 / (n - 1)`.
3. Standard error of the mean pair score: `SE_m = sqrt(v / n)`.
4. Back to a score fraction: `p = m / 2`, so `SE_p = SE_m / 2`.
5. Propagate `SE_p` through the Elo transform. Since
   `Elo(p) = (400/ln 10) * ln(p / (1 - p))`,
   its derivative is `dElo/dp = (400 / ln 10) / (p * (1 - p))`, so
   `SE_Elo = (dElo/dp) * SE_p`.
6. Reported half-width: **`CI = 1.96 * SE_Elo`** (≈ 95% under a normal
   approximation), printed as `+/-CI`.

### When it shows `n/a`

- Fewer than 2 pairs (sample variance undefined).
- The score is at 0% or 100% (`p ∉ (0, 1)`), where the Elo transform — and
  hence its standard error — is undefined.

(A pair set with no spread, e.g. every pair scored exactly even, yields a
finite estimate with `CI = +/-0.0`.)

## How to read it, and caveats

- **Overlapping intervals between two engines mean the difference is not
  established** by this data — collect more games (raise `--mini-matches`, use
  `--concurrency`) to tighten the bars.
- The CI captures **sampling error only** — not the modeling assumption that
  results follow the logistic Elo curve.
- Because the rating is *performance vs the whole field*, the per-pair scores
  come from different opponents; that opponent heterogeneity is folded into the
  variance, which makes the interval somewhat **conservative (wider)** — the
  safe direction for an error bar.
- The `1.96` factor is a normal approximation; it is reasonable for a moderate
  number of pairs and rough for very few.

## Where this lives in the code

- `src/elo.rs` — `Standing::relative_elo()` (point estimate) and
  `Standing::elo_ci95()` (the interval, from `Standing::pair_points`).
- `src/tournament.rs` — accumulates each engine's per-mini-match pair scores
  and renders the `Rel.Elo` / `95% CI` columns.
