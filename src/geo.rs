//! Projections cartographiques (sphère de rayon 1) et utilitaires géodésiques.
use serde::{Deserialize, Serialize};
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Projection {
    Equirectangular,
    Mercator,
    Sinusoidal,
    Mollweide,
    Orthographic,
    AzimuthalEqualArea,
    Stereographic,
    LambertConic,
}

impl Projection {
    pub const ALL: [Projection; 8] = [
        Projection::Equirectangular,
        Projection::Mercator,
        Projection::Sinusoidal,
        Projection::Mollweide,
        Projection::Orthographic,
        Projection::AzimuthalEqualArea,
        Projection::Stereographic,
        Projection::LambertConic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Projection::Equirectangular => "Équirectangulaire (plate carrée)",
            Projection::Mercator => "Mercator",
            Projection::Sinusoidal => "Sinusoïdale",
            Projection::Mollweide => "Mollweide",
            Projection::Orthographic => "Orthographique (globe)",
            Projection::AzimuthalEqualArea => "Azimutale équivalente de Lambert",
            Projection::Stereographic => "Stéréographique",
            Projection::LambertConic => "Conique conforme de Lambert",
        }
    }

    pub fn is_azimuthal(self) -> bool {
        matches!(
            self,
            Projection::Orthographic | Projection::AzimuthalEqualArea | Projection::Stereographic
        )
    }

    /// Projections dont le monde est borné par les méridiens ±180° (découpage en bande).
    pub fn is_banded(self) -> bool {
        matches!(
            self,
            Projection::Equirectangular
                | Projection::Mercator
                | Projection::Sinusoidal
                | Projection::Mollweide
        )
    }

    /// Emprise (largeur, hauteur) projetée du « monde entier » pour le cadrage initial.
    pub fn fit_extent(self) -> (f64, f64) {
        match self {
            Projection::Equirectangular | Projection::Sinusoidal => (2.0 * PI, PI),
            Projection::Mercator => (2.0 * PI, 6.2),
            Projection::Mollweide => (4.0 * 2f64.sqrt(), 2.0 * 2f64.sqrt()),
            Projection::Orthographic => (2.0, 2.0),
            Projection::AzimuthalEqualArea => (4.0, 4.0),
            Projection::Stereographic => (6.0, 6.0),
            Projection::LambertConic => (4.0, 4.0),
        }
    }

    pub fn max_lat(self) -> f64 {
        if self == Projection::Mercator { 85.05 } else { 90.0 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub proj: Projection,
    pub lon0: f64,
    pub lat0: f64,
    pub std1: f64,
    pub std2: f64,
}

#[derive(Clone, Debug)]
pub struct Proj {
    p: Projection,
    l0: f64,
    f0: f64,
    n: f64,
    f: f64,
    rho0: f64,
}

fn tan_q(lat: f64) -> f64 {
    (FRAC_PI_4 + lat / 2.0).tan()
}

impl Proj {
    pub fn new(pa: Params) -> Self {
        let l0 = pa.lon0.to_radians();
        let f0 = pa.lat0.to_radians();
        let (mut n, mut f, mut rho0) = (0.0, 0.0, 0.0);
        if pa.proj == Projection::LambertConic {
            let p1 = pa.std1.clamp(-85.0, 85.0).to_radians();
            let p2 = pa.std2.clamp(-85.0, 85.0).to_radians();
            n = if (p1 - p2).abs() < 1e-6 {
                if p1.sin().abs() < 1e-6 { 1e-6 } else { p1.sin() }
            } else {
                (p1.cos() / p2.cos()).ln() / (tan_q(p2) / tan_q(p1)).ln()
            };
            f = p1.cos() * tan_q(p1).powf(n) / n;
            let fc = f0.clamp(-1.48, 1.48);
            rho0 = f / tan_q(fc).powf(n);
        }
        Proj { p: pa.proj, l0, f0, n, f, rho0 }
    }

    /// Projection directe. `lon` en degrés (peut sortir de [-180,180], continuité conservée).
    /// `clamp` : ramène les points cachés sur le limbe (remplissage des polygones).
    pub fn forward(&self, lon: f64, lat: f64, clamp: bool) -> Option<(f64, f64)> {
        let dl = lon.to_radians() - self.l0;
        let mut phi = lat.to_radians();
        match self.p {
            Projection::Equirectangular => Some((dl, phi)),
            Projection::Mercator => {
                phi = phi.clamp(-85.05f64.to_radians(), 85.05f64.to_radians());
                Some((dl, tan_q(phi).ln()))
            }
            Projection::Sinusoidal => Some((dl * phi.cos(), phi)),
            Projection::Mollweide => {
                let mut t = phi;
                if phi.abs() < FRAC_PI_2 - 1e-7 {
                    for _ in 0..30 {
                        let den = 2.0 + 2.0 * (2.0 * t).cos();
                        if den.abs() < 1e-9 {
                            break;
                        }
                        let d = (2.0 * t + (2.0 * t).sin() - PI * phi.sin()) / den;
                        t -= d;
                        if d.abs() < 1e-10 {
                            break;
                        }
                    }
                }
                let s2 = 2f64.sqrt();
                Some((2.0 * s2 / PI * dl * t.cos(), s2 * t.sin()))
            }
            Projection::Orthographic | Projection::AzimuthalEqualArea | Projection::Stereographic => {
                let (sf, cf) = self.f0.sin_cos();
                let cosc = sf * phi.sin() + cf * phi.cos() * dl.cos();
                let mut x = phi.cos() * dl.sin();
                let mut y = cf * phi.sin() - sf * phi.cos() * dl.cos();
                match self.p {
                    Projection::Orthographic => {
                        if cosc < 0.0 {
                            if !clamp {
                                return None;
                            }
                            let r = x.hypot(y).max(1e-12);
                            x /= r;
                            y /= r;
                        }
                    }
                    Projection::AzimuthalEqualArea => {
                        let k = (2.0 / (1.0 + cosc).max(1e-9)).sqrt();
                        x *= k;
                        y *= k;
                    }
                    _ => {
                        if !clamp && 1.0 + cosc < 0.04 {
                            return None;
                        }
                        let k = 2.0 / (1.0 + cosc).max(0.04);
                        x *= k;
                        y *= k;
                    }
                }
                Some((x, y))
            }
            Projection::LambertConic => {
                phi = phi.clamp(-1.48, 1.48);
                let rho = self.f / tan_q(phi).powf(self.n);
                let th = self.n * dl;
                Some((rho * th.sin(), self.rho0 - rho * th.cos()))
            }
        }
    }

    /// Projection inverse -> (lon, lat) en degrés, lon dans [-180, 180].
    pub fn inverse(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let (lam, phi) = match self.p {
            Projection::Equirectangular => (x, y),
            Projection::Mercator => (x, 2.0 * y.exp().atan() - FRAC_PI_2),
            Projection::Sinusoidal => {
                let c = y.cos();
                if c.abs() < 1e-9 {
                    return None;
                }
                (x / c, y)
            }
            Projection::Mollweide => {
                let s2 = 2f64.sqrt();
                if (y / s2).abs() > 1.0 {
                    return None;
                }
                let t = (y / s2).asin();
                let phi = ((2.0 * t + (2.0 * t).sin()) / PI).clamp(-1.0, 1.0).asin();
                let c = t.cos();
                if c.abs() < 1e-9 {
                    return None;
                }
                (PI * x / (2.0 * s2 * c), phi)
            }
            Projection::Orthographic | Projection::AzimuthalEqualArea | Projection::Stereographic => {
                let rho = x.hypot(y);
                let (sf, cf) = self.f0.sin_cos();
                if rho < 1e-12 {
                    (0.0, self.f0)
                } else {
                    let c = match self.p {
                        Projection::Orthographic => {
                            if rho > 1.0 {
                                return None;
                            }
                            rho.asin()
                        }
                        Projection::AzimuthalEqualArea => {
                            if rho > 2.0 {
                                return None;
                            }
                            2.0 * (rho / 2.0).asin()
                        }
                        _ => 2.0 * (rho / 2.0).atan(),
                    };
                    let (sc, cc) = c.sin_cos();
                    let phi = (cc * sf + y * sc * cf / rho).clamp(-1.0, 1.0).asin();
                    let lam = (x * sc).atan2(rho * cf * cc - y * sf * sc);
                    (lam, phi)
                }
            }
            Projection::LambertConic => {
                let s = self.n.signum();
                let dx = x;
                let dy = self.rho0 - y;
                let rho = s * dx.hypot(dy);
                let th = (s * dx).atan2(s * dy);
                let phi = if rho.abs() < 1e-12 {
                    s * FRAC_PI_2
                } else {
                    2.0 * (self.f / rho).powf(1.0 / self.n).atan() - FRAC_PI_2
                };
                (th / self.n, phi)
            }
        };
        if !lam.is_finite() || !phi.is_finite() || (lam.abs() > PI + 1e-6 && self.p.is_banded()) {
            return None;
        }
        let lon = (lam + self.l0).to_degrees();
        Some((wrap_lon(lon), phi.to_degrees()))
    }
}

pub fn wrap_lon(mut lon: f64) -> f64 {
    while lon > 180.0 {
        lon -= 360.0;
    }
    while lon < -180.0 {
        lon += 360.0;
    }
    lon
}

const R_KM: f64 = 6371.0088;

pub fn haversine_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (l1, p1) = (a.0.to_radians(), a.1.to_radians());
    let (l2, p2) = (b.0.to_radians(), b.1.to_radians());
    let h = ((p2 - p1) / 2.0).sin().powi(2) + p1.cos() * p2.cos() * ((l2 - l1) / 2.0).sin().powi(2);
    2.0 * R_KM * h.sqrt().min(1.0).asin()
}

/// Point à `km` du point (lon,lat) selon le cap `bearing` (degrés).
pub fn destination(lon: f64, lat: f64, bearing: f64, km: f64) -> (f64, f64) {
    let d = km / R_KM;
    let (l1, p1, b) = (lon.to_radians(), lat.to_radians(), bearing.to_radians());
    let p2 = (p1.sin() * d.cos() + p1.cos() * d.sin() * b.cos()).clamp(-1.0, 1.0).asin();
    let l2 = l1 + (b.sin() * d.sin() * p1.cos()).atan2(d.cos() - p1.sin() * p2.sin());
    (l2.to_degrees(), p2.to_degrees())
}

/// Points du grand cercle a -> b (inclus), un point tous les `step_deg` degrés.
/// Les longitudes sont rendues continues à partir de `a`.
pub fn great_circle(a: (f64, f64), b: (f64, f64), step_deg: f64) -> Vec<(f64, f64)> {
    let v = |lon: f64, lat: f64| {
        let (l, p) = (lon.to_radians(), lat.to_radians());
        [p.cos() * l.cos(), p.cos() * l.sin(), p.sin()]
    };
    let (va, vb) = (v(a.0, a.1), v(b.0, b.1));
    let dot = (va[0] * vb[0] + va[1] * vb[1] + va[2] * vb[2]).clamp(-1.0, 1.0);
    let om = dot.acos();
    let n = ((om.to_degrees() / step_deg.max(0.1)).ceil() as usize).max(1);
    let mut out = Vec::with_capacity(n + 1);
    let mut prev = a.0;
    for i in 0..=n {
        let t = i as f64 / n as f64;
        let (p, q) = if om < 1e-9 {
            (1.0 - t, t)
        } else {
            (((1.0 - t) * om).sin() / om.sin(), (t * om).sin() / om.sin())
        };
        let w = [p * va[0] + q * vb[0], p * va[1] + q * vb[1], p * va[2] + q * vb[2]];
        let mut lon = w[1].atan2(w[0]).to_degrees();
        while lon - prev > 180.0 {
            lon -= 360.0;
        }
        while lon - prev < -180.0 {
            lon += 360.0;
        }
        prev = lon;
        out.push((lon, w[2].clamp(-1.0, 1.0).asin().to_degrees()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_projections() {
        for proj in Projection::ALL {
            let pr = Proj::new(Params { proj, lon0: 10.0, lat0: 45.0, std1: 30.0, std2: 60.0 });
            for &(lon, lat) in &[(12.0, 40.0), (-20.0, 55.0), (30.0, 20.0), (0.0, 50.0)] {
                let (x, y) = pr.forward(lon, lat, false).expect("visible");
                let (lo, la) = pr.inverse(x, y).expect("inverse");
                assert!(
                    (lo - lon).abs() < 1e-6 && (la - lat).abs() < 1e-6,
                    "{proj:?} {lon},{lat} -> {lo},{la}"
                );
            }
        }
    }

    #[test]
    fn distances() {
        let d = haversine_km((2.35, 48.85), (-0.12, 51.5)); // Paris-Londres
        assert!((d - 343.0).abs() < 5.0, "{d}");
        let (lo, la) = destination(0.0, 0.0, 90.0, 111.19);
        assert!((lo - 1.0).abs() < 0.01 && la.abs() < 0.01);
    }
}
