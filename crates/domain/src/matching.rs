

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {

    pub id: uuid::Uuid,

    pub lat: f64,

    pub lon: f64,

    pub rating: f64,

    pub level: i32,

    pub current_load: i32,

    pub max_load: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weights {

    pub distance: f64,

    pub rating: f64,

    pub level: f64,

    pub load: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            distance: 1.0,
            rating: 0.5,
            level: 0.3,
            load: 0.2,
        }
    }
}

pub fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R_KM: f64 = 6371.0;
    let to_rad = std::f64::consts::PI / 180.0;
    let dlat = (lat2 - lat1) * to_rad;
    let dlon = (lon2 - lon1) * to_rad;
    let a = (dlat / 2.0).sin().powi(2)
        + (lat1 * to_rad).cos() * (lat2 * to_rad).cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    R_KM * c
}

pub fn score(order_lat: f64, order_lon: f64, c: &Candidate, w: &Weights) -> f64 {
    let dist = haversine_km(order_lat, order_lon, c.lat, c.lon).max(0.001);
    let inv_dist = 1.0 / dist;
    let load_pressure = if c.max_load == 0 {
        0.0
    } else {
        c.current_load as f64 / c.max_load as f64
    };
    w.distance * inv_dist + w.rating * c.rating + w.level * c.level as f64 - w.load * load_pressure
}

pub fn top_n(
    order_lat: f64,
    order_lon: f64,
    candidates: &[Candidate],
    n: usize,
    weights: &Weights,
) -> Vec<(Candidate, f64)> {
    let mut scored: Vec<(Candidate, f64)> = candidates
        .iter()
        .map(|c| (c.clone(), score(order_lat, order_lon, c, weights)))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(n);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn cand(lat: f64, lon: f64, rating: f64, level: i32, load: i32, max: i32) -> Candidate {
        Candidate {
            id: Uuid::new_v4(),
            lat,
            lon,
            rating,
            level,
            current_load: load,
            max_load: max,
        }
    }

    #[test]
    fn haversine_known_distance_vientiane_to_luang_prabang() {

        let d = haversine_km(18.0, 102.0, 19.9, 102.1);
        assert!((200.0..250.0).contains(&d), "got {d}");
    }

    #[test]
    fn haversine_zero_for_same_point() {
        assert!(haversine_km(13.7, 100.5, 13.7, 100.5) < 1e-6);
    }

    #[test]
    fn closer_candidate_scores_higher() {
        let w = Weights::default();
        let c1 = cand(13.7, 100.5, 4.5, 5, 0, 5);
        let c2 = cand(15.0, 105.0, 4.5, 5, 0, 5);
        let s1 = score(13.7, 100.5, &c1, &w);
        let s2 = score(13.7, 100.5, &c2, &w);
        assert!(s1 > s2, "near {s1} should beat far {s2}");
    }

    #[test]
    fn higher_rating_beats_closer_when_distance_small() {

        let w = Weights::default();
        let c1 = cand(13.7, 100.50, 3.0, 3, 0, 5);
        let c2 = cand(13.7, 100.51, 4.9, 5, 0, 5);
        let s1 = score(13.7, 100.50, &c1, &w);
        let s2 = score(13.7, 100.50, &c2, &w);

        assert!(s2 > 0.0);
        assert!(s1 > 0.0);

        let dist1 = haversine_km(13.7, 100.50, 13.7, 100.50);
        let dist2 = haversine_km(13.7, 100.50, 13.7, 100.51);
        assert!(dist2 > dist1);
    }

    #[test]
    fn top_n_returns_at_most_n() {
        let w = Weights::default();
        let cands = vec![
            cand(13.0, 100.0, 4.0, 3, 0, 5),
            cand(13.1, 100.1, 4.5, 4, 1, 5),
            cand(13.2, 100.2, 5.0, 5, 0, 5),
            cand(13.3, 100.3, 3.0, 2, 0, 5),
        ];
        let top = top_n(13.7, 100.5, &cands, 2, &w);
        assert_eq!(top.len(), 2);
    }
}
