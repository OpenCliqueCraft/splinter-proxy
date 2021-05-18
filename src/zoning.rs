/// Generic type for an object that can map a coordinate to a "server ID"
pub trait Zoner {
    /// For now, a "server ID" is just a number
    fn get_zone(&self, vec: &Vector2) -> u64;
}

/// Self explanatory, a 2D vector
pub struct Vector2 {
    pub x: i32,
    pub z: i32,
}

/// Used by BasicZoner, either does or does not contain a location
pub trait Region {
    fn contains(&self, vec: &Vector2) -> bool;
}

/// Rectangular implementation of Region
pub struct SquareRegion {
    lower: Vector2,
    upper: Vector2,
}

impl SquareRegion {
    pub fn new(lower: Vector2, upper: Vector2) -> Box<SquareRegion> {
        Box::new(SquareRegion {
            lower: lower,
            upper: upper,
        })
    }
}

impl Region for SquareRegion {
    fn contains(&self, vec: &Vector2) -> bool {
        let lx = vec.x >= self.lower.x;
        let lz = vec.z >= self.lower.z;
        let ux = vec.x < self.upper.x;
        let uz = vec.z < self.upper.z;

        lx && lz && ux && uz
    }
}

/// Contains a list of regions and maps them to a server ID.
/// Tests in order, returns once it hits a truthy region
pub struct BasicZoner {
    regions: Vec<(u64, Box<dyn Region>)>,
    default: u64,
}

impl BasicZoner {
    pub fn new(regions: Vec<(u64, Box<dyn Region>)>, default: u64) -> BasicZoner {
        BasicZoner {
            regions: regions,
            default: default,
        }
    }
}

impl Zoner for BasicZoner {
    fn get_zone(&self, vec: &Vector2) -> u64 {
        for reg in &self.regions {
            if reg.1.contains(vec) {
                return reg.0;
            }
        }

        self.default
    }
}
