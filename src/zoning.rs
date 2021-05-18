/// Generic type for an object that can map a coordinate to a "server ID"
pub trait Zoner {
    /// For now, a "server ID" is just a number
    fn get_zone(&self, vec: &Vector2) -> u16;
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
    a: Vector2,
    b: Vector2,
}

impl SquareRegion {
    pub fn new(a: Vector2, b: Vector2) -> Box<SquareRegion> {
        Box::new(SquareRegion {
            a: a,
            b: b,
        })
    }
}

impl Region for SquareRegion {
    fn contains(&self, vec: &Vector2) -> bool {
        let b0 = self.a.x <= vec.x;
        let b1 = self.a.z <= vec.z;
        let b2 = self.b.x >= vec.x;
        let b3 = self.b.z >= vec.z;

        b0 && b1 && b2 && b3
    }
}

/// Contains a list of regions and maps them to a server ID.
/// Tests in order, returns once it hits a truthy region
pub struct BasicZoner {
    regions: Vec<(u16, Box<dyn Region>)>,
    default: u16,
}

impl BasicZoner {
    pub fn new(regions: Vec<(u16, Box<dyn Region>)>, default: u16) -> BasicZoner {
        BasicZoner {
            regions: regions,
            default: default,
        }
    }
}

impl Zoner for BasicZoner {
    fn get_zone(&self, vec: &Vector2) -> u16 {
        for reg in &self.regions {
            if reg.1.contains(vec) {
                return reg.0;
            }
        }

        self.default
    }
}
