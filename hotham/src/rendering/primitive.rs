use crate::{
    asset_importer::ImportContext,
    components::TransformMatrix,
    rendering::{material::NO_MATERIAL, vertex::Vertex},
    resources::render_context,
};
use itertools::izip;
use nalgebra::{vector, Point3, Vector3, Vector4};
use render_context::RenderContext;

/// Geometry for a mesh
/// Automatically generated by `gltf_loader`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Primitive {
    /// Offset into the index buffer
    pub index_buffer_offset: u32,
    /// Offset into vertex buffer
    pub vertex_buffer_offset: u32,
    /// Number of vertices
    pub indices_count: u32,
    /// Material used
    pub material_id: u32,
    /// Bounding sphere - used for culling
    pub bounding_sphere: Vector4<f32>,
}

impl Primitive {
    /// Create a new primitive using a list of vertices, indices and a material ID.
    pub fn new(
        vertices: &[Vertex],
        indices: &[u32],
        material_id: u32,
        render_context: &mut RenderContext,
    ) -> Self {
        let primitive = Primitive {
            indices_count: indices.len() as _,
            material_id,
            index_buffer_offset: render_context.resources.index_buffer.len as _,
            vertex_buffer_offset: render_context.resources.vertex_buffer.len as _,
            bounding_sphere: calculate_bounding_sphere(vertices),
        };

        unsafe {
            render_context.resources.index_buffer.append(indices);
            render_context.resources.vertex_buffer.append(vertices);
        }

        primitive
    }

    pub(crate) fn load(
        primitive_data: gltf::Primitive,
        import_context: &mut ImportContext,
        mesh_name: &str,
    ) -> Self {
        let mut indices = Vec::new();
        let mut positions = Vec::new();
        let mut tex_coords = Vec::new();
        let mut normals = Vec::new();
        let mut joint_indices = Vec::new();
        let mut joint_weights = Vec::new();

        let reader = primitive_data.reader(|_| Some(&import_context.buffer));

        // Positions
        for v in reader
            .read_positions()
            .unwrap_or_else(|| panic!("Mesh {} has no positions!", mesh_name))
        {
            positions.push(vector![v[0], v[1], v[2]]);
        }

        // Indices
        if let Some(iter) = reader.read_indices() {
            for i in iter.into_u32() {
                indices.push(i);
            }
        }

        // Normals
        if let Some(iter) = reader.read_normals() {
            for v in iter {
                normals.push(vector![v[0], v[1], v[2]]);
            }
        } else {
            for _ in 0..positions.len() {
                normals.push(vector![0., 0., 0.]);
            }
        }

        if let Some(iter) = reader.read_tex_coords(0) {
            for v in iter.into_f32() {
                tex_coords.push(vector![v[0], v[1]]);
            }
        } else {
            for _ in 0..positions.len() {
                tex_coords.push(vector![0., 0.]);
            }
        }

        if let Some(iter) = reader.read_joints(0) {
            for t in iter.into_u16() {
                joint_indices.push(vector![t[0] as f32, t[1] as f32, t[2] as f32, t[3] as f32]);
            }
        } else {
            for _ in 0..positions.len() {
                joint_indices.push(vector![0., 0., 0., 0.]);
            }
        }

        if let Some(iter) = reader.read_weights(0) {
            for t in iter.into_f32() {
                joint_weights.push(vector![t[0] as f32, t[1] as f32, t[2] as f32, t[3] as f32]);
            }
        } else {
            for _ in 0..positions.len() {
                joint_weights.push(vector![0., 0., 0., 0.]);
            }
        }

        let vertices: Vec<Vertex> =
            izip!(positions, normals, tex_coords, joint_indices, joint_weights)
                .into_iter()
                .map(Vertex::from_zip)
                .collect();

        // All the materials in this glTF file will be imported into the material buffer, so all we need
        // to do is grab the index of this material and add it to the running offset. If we don't do this,
        // importing multiple glTF files will result in sadness, misery, and really ugly looking scenes.
        let material_id = primitive_data.material().index().unwrap_or(NO_MATERIAL) as u32
            + import_context.material_buffer_offset;

        Primitive::new(
            &vertices,
            &indices,
            material_id,
            import_context.render_context,
        )
    }

    /// Get a bounding sphere for the primitive, applying a transform
    pub fn get_bounding_sphere(&self, transform: &TransformMatrix) -> Vector4<f32> {
        let center_in_local: Point3<_> = self.bounding_sphere.xyz().into();
        let center_in_world =
            Point3::<_>::from_homogeneous(transform.0 * center_in_local.to_homogeneous()).unwrap();
        let world_from_local_linear_part = transform.0.fixed_slice::<3, 3>(0, 0);
        let scale = world_from_local_linear_part
            .column(0)
            .magnitude_squared()
            .max(world_from_local_linear_part.column(1).magnitude_squared())
            .max(world_from_local_linear_part.column(2).magnitude_squared())
            .sqrt();
        let radius_in_world = self.bounding_sphere.w * scale;

        [
            center_in_world.x,
            center_in_world.y,
            center_in_world.z,
            radius_in_world,
        ]
        .into()
    }
}

/// Get a bounding sphere for the primitive, used for occlusion culling
pub fn calculate_bounding_sphere(vertices: &[Vertex]) -> Vector4<f32> {
    let points = vertices.iter().map(|v| v.position).collect::<Vec<_>>();
    let num_points = points.len();
    if num_points == 0 {
        return Default::default();
    }

    let mut center = Vector3::zeros();
    for p in &points {
        center += p;
    }

    center /= num_points as f32;
    let mut radius = (points[0] - center).norm_squared();
    for p in points.iter().skip(1) {
        radius = radius.max((p - center).norm_squared());
    }

    radius = next_up(radius.sqrt());

    [center.x, center.y, center.z, radius].into()
}

const TINY_BITS: u32 = 0x1; // Smallest positive f32.
const CLEAR_SIGN_MASK: u32 = 0x7fff_ffff;

fn next_up(n: f32) -> f32 {
    let bits = n.to_bits();
    if n.is_nan() || bits == f32::INFINITY.to_bits() {
        return n;
    }

    let abs = bits & CLEAR_SIGN_MASK;
    let next_bits = if abs == 0 {
        TINY_BITS
    } else if bits == abs {
        bits + 1
    } else {
        bits - 1
    };
    f32::from_bits(next_bits)
}
