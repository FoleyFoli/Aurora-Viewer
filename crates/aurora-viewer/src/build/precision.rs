//! Numerical terrain tools. All heights come from received land patches. A
//! preview owns its two points independently of the parcel-management selection.

use super::REGION_FLAGS_BLOCK_TERRAFORM;
use crate::world::World;
use crate::world::land::owned_by_agent;
use crate::world::terrain::Heightmap;
use aurora_net::build::{BUILD_PARCEL_SEQ, BuildCmd, land_action};
use aurora_net::{ParcelInfo, RegionHandle, parcel_flags};
use glam::{Vec2, Vec3};
use std::sync::Arc;

// Outside the ranges reserved for the build cost and Select land requests.
const PRECISION_PARCEL_SEQ: i32 = BUILD_PARCEL_SEQ + 2000;
// roles_constants.h GP_LAND_ALLOW_EDIT_LAND, distinct from changing the parcel's options.
const ALLOW_EDIT_LAND: u64 = 1 << 23;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PrecisionMode {
    #[default]
    Off,
    Measure,
    SetHeight,
}

#[derive(Debug, Default)]
pub struct PrecisionTerrain {
    pub mode: PrecisionMode,
    pub region: Option<RegionHandle>,
    /// Region-local XY on the 1 m height grid, independent of camera origin.
    pub a: Option<Vec2>,
    pub b: Option<Vec2>,
    /// Absolute height in metres, even when the UI displays a water-relative value.
    pub target_z: f32,
    pub relative_to_water: bool,
    pub message: Option<String>,
    submitted: Option<Submitted>,
    parcel: Option<Arc<ParcelInfo>>,
    parcel_seq: i32,
    request_parcel: bool,
}

#[derive(Debug)]
struct Submitted {
    center: glam::UVec2,
    target_z: f32,
    patch_revision: u64,
}

/// Interpolate on the same SW-NE triangles as world::terrain::build_chunk.
/// Never use placeholder zeroes, including across patch boundaries. At an
/// exact sample or shared edge only the vertices with nonzero weight matter.
fn mesh_height(map: &Heightmap, local: Vec2) -> Option<f32> {
    if !local.is_finite() || local.min_element() < 0.0 || local.x >= map.size_x as f32 || local.y >= map.size_y as f32 {
        return None;
    }
    let base = local.floor();
    let (x, y) = (base.x as u32, base.y as u32);
    let f = local - base;
    let sw = map.received_height(x, y)?;
    let mut z = sw;
    if f.x > f.y {
        z += (f.x - f.y) * (map.received_height(x + 1, y)? - sw);
    } else if f.y > f.x {
        z += (f.y - f.x) * (map.received_height(x, y + 1)? - sw);
    }
    let diagonal = f.min_element();
    if diagonal > 0.0 {
        z += diagonal * (map.received_height(x + 1, y + 1)? - sw);
    }
    z.is_finite().then_some(z)
}

pub(super) fn received_height(world: &World, handle: RegionHandle, local: Vec2) -> Option<f32> {
    mesh_height(&world.regions.get(&handle)?.heightmap, local)
}

/// Check every parcel cell touched by either corner or the interior. A parcel
/// can have holes and disjoint pieces, so even four matching corners do not suffice.
fn contains_area(parcel: &ParcelInfo, width: u32, lo: Vec2, hi: Vec2) -> bool {
    let stride = width.div_ceil(4) as usize;
    let from = (lo / 4.0).floor().as_uvec2();
    let to = (hi / 4.0).floor().as_uvec2();
    (from.y..=to.y).all(|y| {
        (from.x..=to.x).all(|x| {
            let bit = y as usize * stride + x as usize;
            parcel.bitmap.get(bit / 8).is_some_and(|byte| byte & (1 << (bit % 8)) != 0)
        })
    })
}

impl PrecisionTerrain {
    pub fn set_mode(&mut self, mode: PrecisionMode) {
        if self.mode != mode {
            self.clear();
            self.mode = mode;
        }
    }

    pub fn clear(&mut self) {
        self.region = None;
        self.a = None;
        self.b = None;
        self.parcel = None;
        self.submitted = None;
        self.request_parcel = false;
        self.message = None;
    }

    pub fn pick(&mut self, world: &World, hit: Vec3) {
        if self.mode == PrecisionMode::Off {
            return;
        }
        let Some(handle) = world.main_region else { return };
        let Some(region) = world.regions.get(&handle) else { return };
        // Render coordinates are relative to main_region. Check the raw hit as
        // well as its snapped sample so a click across its west edge stays out.
        let local = hit.truncate();
        let point = local.round();
        if !hit.is_finite()
            || local.min_element() < 0.0
            || point.x >= region.heightmap.size_x as f32
            || point.y >= region.heightmap.size_y as f32
        {
            self.message = Some(aurora_i18n::tr!("build-precision-pick-main").into());
            return;
        }
        let Some(height) = mesh_height(&region.heightmap, point) else {
            self.message = Some(aurora_i18n::tr!("build-precision-terrain-pending").into());
            return;
        };
        self.message = None;
        if self.region == Some(handle) && self.a.is_some() && self.b.is_none() {
            self.b = Some(point);
        } else {
            self.clear();
            self.region = Some(handle);
            self.a = Some(point);
            self.target_z = height;
            if self.mode == PrecisionMode::SetHeight {
                self.parcel_seq = PRECISION_PARCEL_SEQ + (self.parcel_seq + 1 - PRECISION_PARCEL_SEQ).rem_euclid(1000);
                self.request_parcel = true;
            }
        }
    }

    /// Request the actual parcel holding A, independent of where the avatar is
    /// standing. Like Build Costs, snap a request inside A's 4 m cell to parcel.
    pub fn take_parcel_request(&mut self) -> Option<BuildCmd> {
        if !std::mem::take(&mut self.request_parcel) {
            return None;
        }
        let cell = (self.a? / 4.0).floor() * 4.0;
        Some(BuildCmd::ParcelRequest {
            handle: self.region?,
            sequence: self.parcel_seq,
            snap: true,
            west: cell.x + 1.0,
            south: cell.y + 1.0,
            east: cell.x + 3.0,
            north: cell.y + 3.0,
        })
    }

    pub fn on_parcel(&mut self, handle: RegionHandle, sequence: i32, parcel: &Arc<ParcelInfo>) {
        if self.mode == PrecisionMode::SetHeight && self.region == Some(handle) && self.parcel_seq == sequence {
            self.parcel = (parcel.request_result == aurora_net::land::PARCEL_RESULT_SUCCESS).then(|| Arc::clone(parcel));
        }
    }

    pub fn sample(&self, world: &World, point: Vec2) -> Option<f32> {
        let handle = self.region.filter(|h| Some(*h) == world.main_region)?;
        received_height(world, handle, point)
    }

    pub fn water(&self, world: &World) -> Option<f32> {
        let handle = world.main_region?;
        let z = world.regions.get(&handle)?.info.as_ref()?.water_height;
        z.is_finite().then_some(z)
    }

    /// Equally spaced samples along the ground. Skip the whole profile if the
    /// region hasn't delivered a crossed patch; a straight interpolation over a
    /// gap would hide the very bumps the profile is intended to reveal.
    pub fn profile(&self, world: &World) -> Option<Vec<f32>> {
        let (a, b) = (self.a?, self.b?);
        let steps = (a.distance(b).ceil() as usize).clamp(1, 512);
        (0..=steps)
            .map(|i| self.sample(world, a.lerp(b, i as f32 / steps as f32)))
            .collect()
    }

    pub fn can_apply(&self, world: &World) -> Result<(), &'static str> {
        self.apply_area(world).map(|_| ())
    }

    fn apply_area(&self, world: &World) -> Result<(RegionHandle, Vec2, Vec2), &'static str> {
        let area_error = aurora_i18n::tr!("build-precision-choose-area");
        let (Some(a), Some(b)) = (self.a, self.b) else {
            return Err(area_error);
        };
        if self.mode != PrecisionMode::SetHeight || !a.is_finite() || !b.is_finite() || a.x == b.x || a.y == b.y {
            return Err(area_error);
        }
        if !self.target_z.is_finite() {
            return Err(aurora_i18n::tr!("build-precision-invalid-height"));
        }
        let stale = aurora_i18n::tr!("build-precision-region-changed");
        let handle = self.region.filter(|h| Some(*h) == world.main_region).ok_or(stale)?;
        let region = world.regions.get(&handle).ok_or(stale)?;
        let map = &region.heightmap;
        let (lo, hi) = (a.min(b), a.max(b));
        if lo.min_element() < 0.0 || hi.x >= map.size_x as f32 || hi.y >= map.size_y as f32 {
            return Err(area_error);
        }
        let pending = aurora_i18n::tr!("build-precision-terrain-pending");
        let info = region.info.as_ref().ok_or(pending)?;
        let from = lo.as_uvec2() / 16;
        let to = hi.as_uvec2() / 16;
        for y in from.y..=to.y {
            for x in from.x..=to.x {
                if map.received_height(x * 16, y * 16).is_none() {
                    return Err(pending);
                }
            }
        }
        let one_parcel = aurora_i18n::tr!("build-precision-one-parcel");
        let parcel = self
            .parcel
            .as_deref()
            .or_else(|| world.known_parcel_at(handle, a.extend(0.0), None))
            .ok_or_else(|| aurora_i18n::tr!("build-precision-parcel-pending"))?;
        if parcel.request_result != aurora_net::land::PARCEL_RESULT_SUCCESS || !contains_area(parcel, map.size_x, lo, hi) {
            return Err(one_parcel);
        }
        let agent = world.agent_rights();
        let manager = agent.god || info.is_estate_manager || (!agent.id.is_nil() && info.owner == agent.id);
        // LLToolBrushLand::canTerraformRegion and parcel/group rights.
        // Permission is checked again by the simulator when it handles ModifyLand.
        if !manager
            && (info.region_flags & REGION_FLAGS_BLOCK_TERRAFORM != 0
                || (parcel.flags & parcel_flags::ALLOW_TERRAFORM == 0 && !owned_by_agent(parcel, &agent, ALLOW_EDIT_LAND)))
        {
            return Err(aurora_i18n::tr!("build-precision-blocked"));
        }
        Ok((handle, lo, hi))
    }

    pub fn submitted(&self) -> bool {
        self.submitted.is_some()
    }

    /// A new patch containing the center is an independent observation, not an
    /// acknowledgement of this command (another builder may be editing too).
    pub fn observed_center(&self, world: &World) -> Option<(f32, f32)> {
        let sent = self.submitted.as_ref()?;
        let handle = self.region.filter(|h| Some(*h) == world.main_region)?;
        let map = &world.regions.get(&handle)?.heightmap;
        let p = sent.center;
        if map.patch_revision(p.x, p.y)? == sent.patch_revision {
            return None;
        }
        Some((map.received_height(p.x, p.y)?, sent.target_z))
    }

    pub fn apply(&mut self, world: &World, force: f32) -> Option<BuildCmd> {
        let (handle, lo, hi) = match self.apply_area(world) {
            Ok(area) => area,
            Err(reason) => {
                self.message = Some(reason.to_owned());
                return None;
            }
        };
        if !force.is_finite() {
            return None;
        }
        let center = ((lo + hi) * 0.5).round().as_uvec2();
        self.submitted = Some(Submitted {
            center,
            target_z: self.target_z,
            patch_revision: world.regions.get(&handle)?.heightmap.patch_revision(center.x, center.y)?,
        });
        self.message = Some(aurora_i18n::tr!("build-precision-sent").into());
        Some(BuildCmd::ModifyLand {
            handle,
            action: land_action::FLATTEN,
            reliable: true,
            brush_size: 1.0,
            seconds: force.clamp(0.1, 100.0) * 0.25,
            height: self.target_z,
            // A parcel ID requests the entire parcel. Use coordinates only.
            parcel_local_id: -1,
            west: lo.x,
            south: lo.y,
            east: hi.x,
            north: hi.y,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_samples_the_rendered_diagonal_and_ignores_unreceived_zeroes() {
        let mut map = Heightmap::new(32, 32);
        let mut data = [0.0; 256];
        data[17] = 4.0;
        map.apply_patch(0, 0, 16, &data);
        assert_eq!(mesh_height(&map, Vec2::new(0.5, 0.5)), Some(2.0));
        assert_eq!(mesh_height(&map, Vec2::new(0.75, 0.25)), Some(1.0));
        assert_eq!(mesh_height(&map, Vec2::new(0.25, 0.75)), Some(1.0));
        assert_eq!(mesh_height(&map, Vec2::splat(15.0)), Some(0.0));
        assert_eq!(mesh_height(&map, Vec2::new(15.1, 0.0)), None);
        assert_eq!(mesh_height(&map, Vec2::new(-0.1, 0.0)), None);
    }

    fn fixture() -> (World, PrecisionTerrain, RegionHandle) {
        let mut world = World::new(Arc::new(crate::scene::avatar::AvatarLibrary::load()));
        for event in crate::demo::events() {
            world.apply(event);
        }
        let handle = world.main_region.unwrap();
        let r = world.regions.get_mut(&handle).unwrap();
        let info = Arc::make_mut(r.info.as_mut().unwrap());
        info.is_estate_manager = false;
        info.owner = uuid::Uuid::from_u128(101);
        info.region_flags &= !REGION_FLAGS_BLOCK_TERRAFORM;
        r.heightmap.apply_patch(0, 0, 16, &[12.0; 256]);
        world.agent_id = uuid::Uuid::from_u128(100);
        world.god_level = 0;
        let mut t = PrecisionTerrain::default();
        t.set_mode(PrecisionMode::SetHeight);
        t.pick(&world, Vec3::new(11.0, 11.0, 999.0));
        let Some(BuildCmd::ParcelRequest { sequence, snap: true, .. }) = t.take_parcel_request() else {
            panic!("parcel query")
        };
        let parcel = Arc::new(ParcelInfo {
            bitmap: vec![255; 512],
            owner_id: world.agent_id,
            ..Default::default()
        });
        t.on_parcel(handle, sequence, &parcel);
        t.pick(&world, Vec3::new(1.0, 1.0, 999.0));
        (world, t, handle)
    }

    #[test]
    fn apply_uses_the_rectangle_and_only_reports_a_fresh_center_patch() {
        let (mut world, mut t, handle) = fixture();
        assert_eq!(t.target_z, 12.0);
        assert_eq!(t.profile(&world).unwrap(), vec![12.0; 16]);
        t.target_z = 14.5;
        let Some(BuildCmd::ModifyLand {
            action,
            height,
            seconds,
            brush_size,
            reliable,
            parcel_local_id,
            west,
            south,
            east,
            north,
            ..
        }) = t.apply(&world, 2.0)
        else {
            panic!("level request")
        };
        assert!(reliable);
        assert_eq!(action, land_action::FLATTEN);
        assert_eq!((height, seconds, brush_size), (14.5, 0.5, 1.0));
        assert_eq!((parcel_local_id, west, south, east, north), (-1, 1.0, 1.0, 11.0, 11.0));
        assert_eq!(t.observed_center(&world), None);
        let map = &mut world.regions.get_mut(&handle).unwrap().heightmap;
        map.apply_patch(1, 0, 16, &[22.0; 256]);
        assert_eq!(t.observed_center(&world), None);
        world
            .regions
            .get_mut(&handle)
            .unwrap()
            .heightmap
            .apply_patch(0, 0, 16, &[14.25; 256]);
        t.target_z = 15.0;
        assert_eq!(t.observed_center(&world), Some((14.25, 14.5)));
        t.clear();
        assert_eq!(t.observed_center(&world), None);
    }

    #[test]
    fn applying_rechecks_rights_and_the_parcel_interior() {
        let (mut world, mut t, handle) = fixture();
        // All four corners belong to the same parcel, but its center does not.
        let parcel = Arc::make_mut(t.parcel.as_mut().unwrap());
        let hole = 64 + 1;
        parcel.bitmap[hole / 8] &= !(1 << (hole % 8));
        assert!(t.apply(&world, 1.0).is_none());
        let parcel = Arc::make_mut(t.parcel.as_mut().unwrap());
        parcel.bitmap.fill(255);
        parcel.owner_id = uuid::Uuid::from_u128(200);
        parcel.flags = 0;
        assert!(t.apply(&world, 1.0).is_none());
        // The group role which bypasses the edit restriction is bit 23. Bit
        // 21 only changes the parcel's edit setting.
        world.groups.groups.push(aurora_net::GroupMembership {
            id: uuid::Uuid::from_u128(200),
            powers: crate::world::land::powers::LAND_EDIT,
            name: String::new(),
            insignia: uuid::Uuid::nil(),
            contribution: 0,
            accept_notices: false,
            list_in_profile: false,
        });
        assert!(t.can_apply(&world).is_err());
        world.groups.groups.last_mut().unwrap().powers = ALLOW_EDIT_LAND;
        assert!(t.can_apply(&world).is_ok());
        world.groups.groups.last_mut().unwrap().powers = 0;
        Arc::make_mut(t.parcel.as_mut().unwrap()).flags = parcel_flags::ALLOW_TERRAFORM;
        assert!(t.can_apply(&world).is_ok());
        let region = world.regions.get_mut(&handle).unwrap();
        Arc::make_mut(region.info.as_mut().unwrap()).region_flags |= REGION_FLAGS_BLOCK_TERRAFORM;
        assert!(t.apply(&world, 1.0).is_none());
        Arc::make_mut(world.regions.get_mut(&handle).unwrap().info.as_mut().unwrap()).is_estate_manager = true;
        assert!(t.can_apply(&world).is_ok());
        world.main_region = None;
        assert!(t.apply(&world, 1.0).is_none());
    }

    #[test]
    fn degenerate_incomplete_and_nonfinite_work_areas_cannot_be_applied() {
        let (mut world, mut t, handle) = fixture();
        t.b = t.a;
        assert!(t.apply(&world, 1.0).is_none());
        t.b = Some(Vec2::new(1.0, 11.0));
        assert!(t.apply(&world, 1.0).is_none());
        t.b = Some(Vec2::ONE);
        t.target_z = f32::NAN;
        assert!(t.apply(&world, 1.0).is_none());
        t.target_z = 20.0;
        // A and B are loaded, but a patch between them is missing.
        let map = &mut world.regions.get_mut(&handle).unwrap().heightmap;
        *map = Heightmap::new(256, 256);
        map.apply_patch(0, 0, 16, &[10.0; 256]);
        map.apply_patch(2, 0, 16, &[10.0; 256]);
        t.b = Some(Vec2::new(34.0, 4.0));
        assert!(t.profile(&world).is_none());
        assert!(t.apply(&world, 1.0).is_none());
    }

    #[test]
    fn new_selection_drops_old_replies_and_measure_never_sends_commands() {
        let (world, mut t, handle) = fixture();
        let old_seq = t.parcel_seq;
        let old = t.parcel.as_ref().unwrap().clone();
        t.pick(&world, Vec3::new(5.0, 5.0, 0.0));
        assert_eq!(t.b, None);
        t.on_parcel(handle, old_seq, &old);
        assert!(t.parcel.is_none());
        t.set_mode(PrecisionMode::Measure);
        t.pick(&world, Vec3::new(6.0, 6.0, 0.0));
        t.pick(&world, Vec3::new(8.0, 9.0, 0.0));
        assert!(t.take_parcel_request().is_none());
        assert!(t.apply(&world, 1.0).is_none());
    }
}
