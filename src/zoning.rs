/// Self explanatory, a 2D vector
pub struct Vector2 {
    pub x: i32,
    pub z: i32,
}

/// A region that can make a location to a server
pub struct Region {
    pub id: u64,
    pub region: RegionType,
}

/// A type of region
pub enum RegionType {
    /// Rectangular region
    Square { lower: Vector2, upper: Vector2 },
}
impl RegionType {
    pub fn contains(&self, vec: &Vector2) -> bool {
        match self {
            RegionType::Square {
                lower,
                upper,
            } => vec.x >= lower.x && vec.z >= lower.z && vec.x < upper.x && vec.z < upper.z,
        }
    }
}

/// Contains a list of regions and maps them to a server ID.
/// Tests in order, returns once it hits a truthy region
pub struct Zoner {
    pub regions: Vec<Region>,
    pub default: u64,
}

impl Zoner {
    pub fn get_zone(&self, vec: &Vector2) -> u64 {
        for reg in &self.regions {
            if reg.region.contains(vec) {
                return reg.id;
            }
        }

        self.default
    }
}
