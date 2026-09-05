// One-shot CPU reference for the native VS boundary. This uses the retired
// frame's GPU-authored matrices and compaction, so it cannot validate the
// transformer itself. It never changes render inputs or retains allocations.

struct PicassoVueCompareInputs<'a> {
    vertices: &'a [u8],
    indices: &'a [u8],
    camera: &'a [u8],
    instances: &'a [u8],
    compacted: &'a [u8],
    indirect: &'a [u8],
    vertex_stride: usize,
}

#[derive(Debug, PartialEq)]
struct PicassoVueCompareError {
    reason: &'static str,
    record: Option<usize>,
}

#[derive(Debug)]
struct PicassoVueMismatch {
    record: usize,
    slot: usize,
    instance: usize,
    vertex: usize,
    actual_bits: [u32; 4],
    expected: [f64; 4],
    tolerance: [f64; 4],
}

#[derive(Debug)]
struct PicassoVueComparison {
    records: usize,
    mismatched_records: usize,
    mismatched_components: usize,
    nonfinite_actual_records: usize,
    zero_actual_records: usize,
    zero_mismatched_records: usize,
    expected_inside_records: usize,
    mismatched_expected_inside_records: usize,
    zero_mismatched_expected_inside_records: usize,
    max_abs_error: f64,
    max_error_over_tolerance: f64,
    first_mismatch: Option<PicassoVueMismatch>,
    indirect: [u32; 5],
}

fn picasso_vue_compare_error(reason: &'static str) -> PicassoVueCompareError {
    PicassoVueCompareError {
        reason,
        record: None,
    }
}

fn picasso_vue_byte_range(bytes: &[u8], offset: usize, length: usize) -> Option<&[u8]> {
    bytes.get(offset..offset.checked_add(length)?)
}

fn picasso_vue_read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(picasso_vue_byte_range(bytes, offset, 4)?.try_into().ok()?))
}

fn picasso_vue_read_matrix(bytes: &[u8], offset: usize) -> Option<[f64; 16]> {
    let bytes = picasso_vue_byte_range(bytes, offset, 64)?;
    let mut matrix = [0.0; 16];
    for (index, value) in matrix.iter_mut().enumerate() {
        *value = f32::from_bits(picasso_vue_read_u32(bytes, index * 4)?) as f64;
        if !value.is_finite() {
            return None;
        }
    }
    Some(matrix)
}

fn picasso_vue_gpu_range(
    allocation_gpu: u64,
    allocation_bytes: usize,
    gpu: u64,
    bytes: usize,
) -> Option<(usize, usize)> {
    allocation_gpu.checked_add(allocation_bytes as u64)?;
    gpu.checked_add(bytes as u64)?;
    let offset = usize::try_from(gpu.checked_sub(allocation_gpu)?).ok()?;
    let end = offset.checked_add(bytes)?;
    (end <= allocation_bytes && end <= isize::MAX as usize).then_some((offset, bytes))
}

fn compare_picasso_vue_records(
    records: &[u32],
    inputs: PicassoVueCompareInputs<'_>,
) -> Result<PicassoVueComparison, PicassoVueCompareError> {
    let invalid = picasso_vue_compare_error;
    if inputs.vertex_stride < 12 || inputs.vertex_stride % 4 != 0 {
        return Err(invalid("vertex-stride"));
    }
    let mut indirect = [0; 5];
    for (index, value) in indirect.iter_mut().enumerate() {
        *value = picasso_vue_read_u32(inputs.indirect, index * 4)
            .ok_or_else(|| invalid("indirect-range"))?;
    }
    let [
        index_count,
        instance_count,
        first_index,
        base_vertex_bits,
        first_instance,
    ] = indirect;
    let expected_records = (index_count as usize).checked_mul(instance_count as usize);
    if index_count == 0
        || index_count % 3 != 0
        || instance_count == 0
        || records.len() % 24 != 0
        || expected_records != Some(records.len() / 8)
    {
        return Err(invalid("nonempty-trilist-record-count"));
    }
    let index_start = (first_index as usize)
        .checked_mul(4)
        .ok_or_else(|| invalid("index-range"))?;
    let index_bytes = (index_count as usize)
        .checked_mul(4)
        .ok_or_else(|| invalid("index-range"))?;
    let indices = picasso_vue_byte_range(inputs.indices, index_start, index_bytes)
        .ok_or_else(|| invalid("index-range"))?;
    let slot_start = (first_instance as usize)
        .checked_mul(4)
        .ok_or_else(|| invalid("compacted-range"))?;
    let slot_bytes = (instance_count as usize)
        .checked_mul(4)
        .ok_or_else(|| invalid("compacted-range"))?;
    let compacted = picasso_vue_byte_range(inputs.compacted, slot_start, slot_bytes)
        .ok_or_else(|| invalid("compacted-range"))?;
    // WGSL Camera.view_proj follows view and proj, each 64 bytes.
    let camera = picasso_vue_read_matrix(inputs.camera, 128)
        .ok_or_else(|| invalid("camera-range-or-nonfinite"))?;
    let mut result = PicassoVueComparison {
        records: records.len() / 8,
        mismatched_records: 0,
        mismatched_components: 0,
        nonfinite_actual_records: 0,
        zero_actual_records: 0,
        zero_mismatched_records: 0,
        expected_inside_records: 0,
        mismatched_expected_inside_records: 0,
        zero_mismatched_expected_inside_records: 0,
        max_abs_error: 0.0,
        max_error_over_tolerance: 0.0,
        first_mismatch: None,
        indirect,
    };
    let base_vertex = base_vertex_bits as i32 as i64;
    // Both loops are bounded by the complete captured record count. No
    // allocation depends on any indirect field or GPU-authored index.
    for (instance_number, slot) in compacted.chunks_exact(4).enumerate() {
        let first_record = instance_number * index_count as usize;
        let instance = u32::from_le_bytes(slot.try_into().unwrap()) as usize;
        let instance_error = |reason| PicassoVueCompareError {
            reason,
            record: Some(first_record),
        };
        let matrix_offset = instance
            .checked_mul(208)
            .ok_or_else(|| instance_error("instance-range"))?;
        // Require the whole ABI row to exist, not just its leading matrix.
        let row = picasso_vue_byte_range(inputs.instances, matrix_offset, 208)
            .ok_or_else(|| instance_error("instance-range"))?;
        let model = picasso_vue_read_matrix(row, 0)
            .ok_or_else(|| instance_error("instance-matrix-nonfinite"))?;
        for (corner, index) in indices.chunks_exact(4).enumerate() {
            let record = first_record + corner;
            let error = |reason| PicassoVueCompareError {
                reason,
                record: Some(record),
            };
            let indexed_vertex = u32::from_le_bytes(index.try_into().unwrap()) as i64 + base_vertex;
            let vertex =
                usize::try_from(indexed_vertex).map_err(|_| error("vertex-index-negative"))?;
            let vertex_offset = vertex
                .checked_mul(inputs.vertex_stride)
                .ok_or_else(|| error("vertex-range"))?;
            let source =
                picasso_vue_byte_range(inputs.vertices, vertex_offset, inputs.vertex_stride)
                    .ok_or_else(|| error("vertex-range"))?;
            let mut position = [0.0, 0.0, 0.0, 1.0];
            for (axis, value) in position[..3].iter_mut().enumerate() {
                *value = f32::from_bits(picasso_vue_read_u32(source, axis * 4).unwrap()) as f64;
            }
            if position.iter().any(|value| !value.is_finite()) {
                return Err(error("source-position-nonfinite"));
            }
            // Column-major WGSL: clip = view_proj * (model * [XYZ, 1]).
            // f64 avoids reproducing the GPU's exact f32 instruction order.
            // The sum of absolute products propagates cancellation-sensitive
            // error through both dot products. 32 eps plus an absolute floor
            // allows f32 multiply/add versus FMA/reassociation differences;
            // this is a diagnostic tolerance, not bitwise shader validation.
            let mut world = [0.0; 4];
            let mut world_magnitude = [0.0; 4];
            for row in 0..4 {
                for column in 0..4 {
                    let term = model[column * 4 + row] * position[column];
                    world[row] += term;
                    world_magnitude[row] += term.abs();
                }
            }
            if world.iter().any(|value| !(*value as f32).is_finite()) {
                return Err(error("expected-world-nonfinite-f32"));
            }
            let mut expected = [0.0; 4];
            let mut tolerance = [0.0; 4];
            for row in 0..4 {
                let mut magnitude = 0.0;
                for column in 0..4 {
                    let coefficient = camera[column * 4 + row];
                    expected[row] += coefficient * world[column];
                    magnitude += coefficient.abs() * world_magnitude[column];
                }
                tolerance[row] = 1.0e-6 + 32.0 * f32::EPSILON as f64 * magnitude;
            }
            // tools/wgsl-spv uses Naga's default ADJUST_COORDINATE_SPACE.
            // Both checked PBR and authored-UV VS executables negate the
            // final camera Y dot product; X, Z and W are unchanged. Compare
            // the lowered pre-clip VUE convention, not WGSL Position Y.
            expected[1] = -expected[1];
            if expected.iter().any(|value| !(*value as f32).is_finite()) {
                return Err(error("expected-clip-nonfinite-f32"));
            }
            let actual_bits: [u32; 4] = records[record * 8 + 4..record * 8 + 8].try_into().unwrap();
            let mut mismatched = false;
            let mut nonfinite = false;
            for axis in 0..4 {
                let actual = f32::from_bits(actual_bits[axis]) as f64;
                let difference = if actual.is_finite() {
                    (actual - expected[axis]).abs()
                } else {
                    nonfinite = true;
                    f64::INFINITY
                };
                result.max_abs_error = result.max_abs_error.max(difference);
                result.max_error_over_tolerance = result
                    .max_error_over_tolerance
                    .max(difference / tolerance[axis]);
                if difference > tolerance[axis] {
                    result.mismatched_components += 1;
                    mismatched = true;
                }
            }
            result.nonfinite_actual_records += usize::from(nonfinite);
            result.mismatched_records += usize::from(mismatched);
            // Count the actual record sets, rather than inferring that equal
            // aggregate W/mismatch counts refer to the same vertices. Both
            // signs of zero represent a degenerate homogeneous position.
            let zero = actual_bits.iter().all(|bits| bits & 0x7FFF_FFFF == 0);
            let [x, y, z, w] = expected;
            let expected_inside =
                w > 0.0 && x >= -w && x <= w && y >= -w && y <= w && z >= 0.0 && z <= w;
            result.zero_actual_records += usize::from(zero);
            result.zero_mismatched_records += usize::from(zero && mismatched);
            result.expected_inside_records += usize::from(expected_inside);
            result.mismatched_expected_inside_records += usize::from(mismatched && expected_inside);
            result.zero_mismatched_expected_inside_records +=
                usize::from(zero && mismatched && expected_inside);
            if mismatched && result.first_mismatch.is_none() {
                result.first_mismatch = Some(PicassoVueMismatch {
                    record,
                    slot: first_instance as usize + instance_number,
                    instance,
                    vertex,
                    actual_bits,
                    expected,
                    tolerance,
                });
            }
        }
    }
    Ok(result)
}

fn picasso_vue_resident_bytes(
    buffer: &ResidentRenderBuffer,
    gpu: u64,
    bytes: usize,
) -> Result<&[u8], PicassoVueCompareError> {
    let (offset, bytes) = picasso_vue_gpu_range(buffer.gpu_base, buffer.storage_bytes, gpu, bytes)
        .filter(|_| !buffer.storage_virt.is_null())
        .ok_or_else(|| picasso_vue_compare_error("resident-readback-range"))?;
    let pointer = unsafe { buffer.storage_virt.add(offset) };
    crate::intel::dma_flush(pointer, bytes);
    Ok(unsafe { core::slice::from_raw_parts(pointer, bytes) })
}

fn log_picasso_vue_comparison_after_retire(resident: &ResidentChurnForward, records: &[u32]) {
    // The caller holds the frame's ownership through retirement/readback.
    // Every slice is checked against its resident allocation before access.
    let comparison = (|| {
        let [camera_binding, instance_binding, compacted_binding] =
            resident.native_vf.vs_storage_bindings;
        if camera_binding.byte_len < 192
            || instance_binding.byte_len == 0
            || instance_binding.byte_len % 208 != 0
            || compacted_binding.byte_len == 0
            || compacted_binding.byte_len % 4 != 0
        {
            return Err(picasso_vue_compare_error("shader-binding-length"));
        }
        let vertices = picasso_vue_resident_bytes(
            &resident.geometry,
            resident.vertex_gpu_addr,
            resident.vertex_bytes as usize,
        )?;
        let indices = picasso_vue_resident_bytes(
            &resident.geometry,
            resident.index_gpu_addr,
            resident.index_bytes as usize,
        )?;
        // GPU allocations include page padding. Match the logical ranges the
        // VS binds so a corrupted index into that padding cannot be accepted
        // as a valid reference row/slot. Binding-relative GPU offsets are
        // checked against their owning allocations by the readback helper.
        let camera = picasso_vue_resident_bytes(
            &resident.camera,
            camera_binding.gpu_addr,
            camera_binding.byte_len as usize,
        )?;
        let instances = picasso_vue_resident_bytes(
            &resident.instances,
            instance_binding.gpu_addr,
            instance_binding.byte_len as usize,
        )?;
        let compacted = picasso_vue_resident_bytes(
            &resident.compacted_indices,
            compacted_binding.gpu_addr,
            compacted_binding.byte_len as usize,
        )?;
        let indirect = picasso_vue_resident_bytes(
            &resident.indirect_args,
            resident.indirect_args.gpu_base,
            20,
        )?;
        compare_picasso_vue_records(
            records,
            PicassoVueCompareInputs {
                vertices,
                indices,
                camera,
                instances,
                compacted,
                indirect,
                vertex_stride: resident.vertex_stride as usize,
            },
        )
    })();
    match comparison {
        Ok(result) => {
            crate::log_info!(target: "render";
                "picasso-vue-compare: valid_inputs=1 records={} mismatched_records={} mismatched_components={} nonfinite_actual_records={} max_abs_error={:?} max_error_over_tolerance={:?} indirect_count_instances_firstindex_basevertex_firstinstance={:?} base_vertex_signed={} source=same-retired-frame-camera-gpu-matrices-compaction-indexed-mesh reference=f64-column-major-two-matvec/naga-position-y-negated tolerance=1e-6+32*f32eps*propagated-absolute-products does_not_prove=transformer-correctness-or-raster-target-output\n",
                result.records, result.mismatched_records, result.mismatched_components,
                result.nonfinite_actual_records, result.max_abs_error, result.max_error_over_tolerance,
                result.indirect, result.indirect[3] as i32,
            );
            crate::log_info!(target: "render";
                "picasso-vue-compare-coverage: zero_actual_records={} zero_mismatched_records={} expected_canonical_inside_records={} mismatched_expected_canonical_inside_records={} zero_mismatched_expected_canonical_inside_records={} classification=cpu-reference-vertices-not-triangle-visibility-or-raster-output\n",
                result.zero_actual_records, result.zero_mismatched_records,
                result.expected_inside_records, result.mismatched_expected_inside_records,
                result.zero_mismatched_expected_inside_records,
            );
            if let Some(first) = result.first_mismatch {
                crate::log_info!(target: "render";
                    "picasso-vue-mismatch: first_record={} triangle={} corner={} compacted_slot={} instance_id={} vertex_index={} actual_xyzw_hex={:08X?} expected_xyzw={:?} tolerance_xyzw={:?}\n",
                    first.record, first.record / 3, first.record % 3, first.slot, first.instance,
                    first.vertex, first.actual_bits, first.expected, first.tolerance,
                );
            }
        }
        Err(error) => {
            crate::log_warn!(target: "render";
                "picasso-vue-compare: valid_inputs=0 comparison=aborted reason={} first_record={:?} proof=none\n",
                error.reason, error.record,
            );
        }
    }
}

#[cfg(test)]
mod picasso_vue_compare_tests {
    use super::*;

    struct Fixture {
        vertices: [u8; 144],
        indices: [u8; 16],
        camera: [u8; 192],
        instances: [u8; 416],
        compacted: [u8; 12],
        indirect: [u8; 20],
        records: [u32; 48],
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_f32s(bytes: &mut [u8], offset: usize, values: &[f32]) {
        for (index, value) in values.iter().enumerate() {
            put_u32(bytes, offset + index * 4, value.to_bits());
        }
    }

    impl Fixture {
        fn new() -> Self {
            let mut fixture = Self {
                vertices: [0; 144],
                indices: [0; 16],
                camera: [0; 192],
                instances: [0; 416],
                compacted: [0; 12],
                indirect: [0; 20],
                records: [0; 48],
            };
            for (index, position) in [[1.0, 2.0, 3.0], [-2.0, 1.0, 4.0], [0.0, -1.0, 2.0]]
                .iter()
                .enumerate()
            {
                put_f32s(&mut fixture.vertices, index * 48, position);
            }
            // Skip the first index, apply signed base_vertex=-2, giving A,C,B.
            for (index, value) in [999, 2, 4, 3].into_iter().enumerate() {
                put_u32(&mut fixture.indices, index * 4, value);
            }
            // Skip compacted slot0, then render matrix1 followed by matrix0.
            for (index, value) in [u32::MAX, 1, 0].into_iter().enumerate() {
                put_u32(&mut fixture.compacted, index * 4, value);
            }
            for (index, value) in [3, 2, 1, (-2i32) as u32, 1].into_iter().enumerate() {
                put_u32(&mut fixture.indirect, index * 4, value);
            }
            // Matrix0: translate (10,20,30). Matrix1: (-2y+1,3x-2,4z+5).
            put_f32s(
                &mut fixture.instances,
                0,
                &[
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 10.0, 20.0, 30.0,
                    1.0,
                ],
            );
            put_f32s(
                &mut fixture.instances,
                208,
                &[
                    0.0, 3.0, 0.0, 0.0, -2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0, 1.0, -2.0, 5.0,
                    1.0,
                ],
            );
            // Camera: (2x+z, -y+3, z+4, z/2+1), including perspective W.
            put_f32s(
                &mut fixture.camera,
                128,
                &[
                    2.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.5, 0.0, 3.0, 4.0, 1.0,
                ],
            );
            // Independently calculated references include the baked Naga Y
            // negation; do not use a shared matrix helper to generate them.
            for (record, position) in [
                [11.0f32, -2.0, 21.0, 9.5],
                [19.0, -5.0, 17.0, 7.5],
                [19.0, -11.0, 25.0, 11.5],
                [55.0, 19.0, 37.0, 17.5],
                [52.0, 16.0, 36.0, 17.0],
                [50.0, 18.0, 38.0, 18.0],
            ]
            .into_iter()
            .enumerate()
            {
                for (axis, value) in position.into_iter().enumerate() {
                    fixture.records[record * 8 + 4 + axis] = value.to_bits();
                }
            }
            fixture
        }

        fn inputs(&self) -> PicassoVueCompareInputs<'_> {
            PicassoVueCompareInputs {
                vertices: &self.vertices,
                indices: &self.indices,
                camera: &self.camera,
                instances: &self.instances,
                compacted: &self.compacted,
                indirect: &self.indirect,
                vertex_stride: 48,
            }
        }
    }

    #[test]
    fn same_frame_reference_uses_all_indirect_fields_and_column_major_matrices() {
        let fixture = Fixture::new();
        let result = compare_picasso_vue_records(&fixture.records, fixture.inputs()).unwrap();
        assert_eq!(result.records, 6);
        assert_eq!(result.indirect, [3, 2, 1, (-2i32) as u32, 1]);
        assert_eq!(result.mismatched_records, 0);
        assert_eq!(result.mismatched_components, 0);
        assert_eq!(result.max_abs_error, 0.0);
        assert!(result.first_mismatch.is_none());
    }

    #[test]
    fn finite_corruption_is_located_while_float_rounding_is_tolerated() {
        let mut fixture = Fixture::new();
        fixture.records[4] = (11.0f32 + 0.00001).to_bits();
        assert_eq!(
            compare_picasso_vue_records(&fixture.records, fixture.inputs())
                .unwrap()
                .mismatched_records,
            0
        );
        fixture.records[2 * 8 + 4] = 19.25f32.to_bits();
        let result = compare_picasso_vue_records(&fixture.records, fixture.inputs()).unwrap();
        assert_eq!((result.mismatched_records, result.mismatched_components), (1, 1));
        assert_eq!(result.nonfinite_actual_records, 0);
        let first = result.first_mismatch.unwrap();
        assert_eq!((first.record, first.slot, first.instance, first.vertex), (2, 1, 1, 1));
        assert_eq!(first.expected, [19.0, -11.0, 25.0, 11.5]);
        assert_eq!(first.actual_bits[0], 19.25f32.to_bits());
        assert_eq!(result.max_abs_error, 0.25);
        assert!(result.max_error_over_tolerance > 100.0);
    }

    #[test]
    fn zero_vectors_and_in_volume_mismatches_are_counted_by_record() {
        let mut fixture = Fixture::new();
        put_f32s(
            &mut fixture.camera,
            128,
            &[
                0.01, 0.0, 0.0, 0.0, 0.0, 0.01, 0.0, 0.0, 0.0, 0.0, 0.01, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        );
        for (record, position) in [
            [-0.03f32, -0.01, 0.17, 1.0],
            [0.03, 0.02, 0.13, 1.0],
            [-0.01, 0.08, 0.21, 1.0],
            [0.11, -0.22, 0.33, 1.0],
            [0.10, -0.19, 0.32, 1.0],
            [0.08, -0.21, 0.34, 1.0],
        ]
        .into_iter()
        .enumerate()
        {
            for (axis, value) in position.into_iter().enumerate() {
                fixture.records[record * 8 + 4 + axis] = value.to_bits();
            }
        }
        let result = compare_picasso_vue_records(&fixture.records, fixture.inputs()).unwrap();
        assert_eq!(result.mismatched_records, 0);
        assert_eq!(result.expected_inside_records, 6);
        fixture.records[4..8].fill(0);
        fixture.records[12..16].fill((-0.0f32).to_bits());
        fixture.records[20] = f32::NAN.to_bits();
        fixture.records[28] = 0.20f32.to_bits();
        let result = compare_picasso_vue_records(&fixture.records, fixture.inputs()).unwrap();
        assert_eq!(result.mismatched_records, 4);
        assert_eq!(result.zero_actual_records, 2);
        assert_eq!(result.zero_mismatched_records, 2);
        assert_eq!(result.nonfinite_actual_records, 1);
        assert_eq!(result.mismatched_expected_inside_records, 4);
        assert_eq!(result.zero_mismatched_expected_inside_records, 2);

        // Degenerate reference matrices can also produce zero. A zero-vector
        // counter must not silently turn that into a VS mismatch or visibility.
        fixture.camera[128..192].fill(0);
        fixture.records.fill(0);
        let result = compare_picasso_vue_records(&fixture.records, fixture.inputs()).unwrap();
        assert_eq!(result.zero_actual_records, 6);
        assert_eq!(result.zero_mismatched_records, 0);
        assert_eq!(result.expected_inside_records, 0);
        assert_eq!(result.mismatched_expected_inside_records, 0);
        assert_eq!(result.zero_mismatched_expected_inside_records, 0);
    }

    #[test]
    fn truncated_buffers_and_invalid_indices_abort_before_out_of_range_access() {
        let fixture = Fixture::new();
        for (case, expected) in [
            (0, "vertex-range"),
            (1, "index-range"),
            (2, "camera-range-or-nonfinite"),
            (3, "instance-range"),
            (4, "compacted-range"),
            (5, "indirect-range"),
            (6, "vertex-stride"),
        ] {
            let mut inputs = fixture.inputs();
            match case {
                0 => inputs.vertices = &fixture.vertices[..143],
                1 => inputs.indices = &fixture.indices[..15],
                2 => inputs.camera = &fixture.camera[..191],
                3 => inputs.instances = &fixture.instances[..415],
                4 => inputs.compacted = &fixture.compacted[..11],
                5 => inputs.indirect = &fixture.indirect[..19],
                _ => inputs.vertex_stride = 8,
            }
            assert_eq!(
                compare_picasso_vue_records(&fixture.records, inputs)
                    .unwrap_err()
                    .reason,
                expected
            );
        }
        for (offset, value, expected) in [
            (8, u32::MAX, "index-range"),
            (12, (-3i32) as u32, "vertex-index-negative"),
            (12, i32::MAX as u32, "vertex-range"),
            (16, u32::MAX, "compacted-range"),
        ] {
            let mut changed = Fixture::new();
            put_u32(&mut changed.indirect, offset, value);
            assert_eq!(
                compare_picasso_vue_records(&changed.records, changed.inputs())
                    .unwrap_err()
                    .reason,
                expected
            );
        }
        let mut changed = Fixture::new();
        put_u32(&mut changed.compacted, 4, u32::MAX);
        assert_eq!(
            compare_picasso_vue_records(&changed.records, changed.inputs())
                .unwrap_err()
                .reason,
            "instance-range"
        );
        put_u32(&mut changed.compacted, 4, 1);
        put_u32(&mut changed.indices, 4, u32::MAX);
        assert_eq!(
            compare_picasso_vue_records(&changed.records, changed.inputs())
                .unwrap_err()
                .reason,
            "vertex-range"
        );
    }

    #[test]
    fn empty_or_partial_capture_cannot_pass_a_reference_comparison() {
        let mut fixture = Fixture::new();
        assert_eq!(
            compare_picasso_vue_records(&fixture.records[..47], fixture.inputs())
                .unwrap_err()
                .reason,
            "nonempty-trilist-record-count"
        );
        put_u32(&mut fixture.indirect, 4, 0);
        assert_eq!(
            compare_picasso_vue_records(&[], fixture.inputs())
                .unwrap_err()
                .reason,
            "nonempty-trilist-record-count"
        );
        put_u32(&mut fixture.indirect, 4, 2);
        put_u32(&mut fixture.indirect, 0, 4);
        assert_eq!(
            compare_picasso_vue_records(&fixture.records, fixture.inputs())
                .unwrap_err()
                .reason,
            "nonempty-trilist-record-count"
        );
    }

    #[test]
    fn shader_visible_limits_exclude_mapped_allocation_padding() {
        let mut fixture = Fixture::new();
        let mut padded_instances = [0; 4096];
        padded_instances[..fixture.instances.len()].copy_from_slice(&fixture.instances);
        // Physically mapped row2 even contains a plausible matrix, but the
        // shader binding exposes only two rows. It must not validate this ID.
        padded_instances[416..624].copy_from_slice(&fixture.instances[208..416]);
        put_u32(&mut fixture.compacted, 4, 2);
        let mut inputs = fixture.inputs();
        inputs.instances = &padded_instances[..416];
        assert_eq!(
            compare_picasso_vue_records(&fixture.records, inputs)
                .unwrap_err()
                .reason,
            "instance-range"
        );
        put_u32(&mut fixture.compacted, 4, 1);
        let mut padded_compacted = [0; 4096];
        padded_compacted[..fixture.compacted.len()].copy_from_slice(&fixture.compacted);
        put_u32(&mut fixture.indirect, 16, 2);
        let mut inputs = fixture.inputs();
        inputs.compacted = &padded_compacted[..12];
        assert_eq!(
            compare_picasso_vue_records(&fixture.records, inputs)
                .unwrap_err()
                .reason,
            "compacted-range"
        );
    }

    #[test]
    fn nonfinite_sources_abort_and_nonfinite_gpu_positions_are_mismatches() {
        for (case, expected) in [
            (0, "camera-range-or-nonfinite"),
            (1, "instance-matrix-nonfinite"),
            (2, "source-position-nonfinite"),
            (3, "expected-world-nonfinite-f32"),
            (4, "expected-clip-nonfinite-f32"),
        ] {
            let mut fixture = Fixture::new();
            match case {
                0 => put_u32(&mut fixture.camera, 128, f32::NAN.to_bits()),
                1 => put_u32(&mut fixture.instances, 208, f32::INFINITY.to_bits()),
                2 => put_u32(&mut fixture.vertices, 0, f32::NAN.to_bits()),
                3 => put_u32(&mut fixture.instances, 208 + 16, f32::MAX.to_bits()),
                _ => put_u32(&mut fixture.camera, 128, f32::MAX.to_bits()),
            }
            assert_eq!(
                compare_picasso_vue_records(&fixture.records, fixture.inputs())
                    .unwrap_err()
                    .reason,
                expected
            );
        }
        let mut fixture = Fixture::new();
        fixture.records[4] = f32::NAN.to_bits();
        fixture.records[5] = f32::INFINITY.to_bits();
        let result = compare_picasso_vue_records(&fixture.records, fixture.inputs()).unwrap();
        assert_eq!((result.mismatched_records, result.mismatched_components), (1, 2));
        assert_eq!(result.nonfinite_actual_records, 1);
        assert_eq!(result.max_abs_error, f64::INFINITY);
    }

    #[test]
    fn gpu_subranges_reject_underflow_overflow_and_allocation_crossing() {
        assert_eq!(picasso_vue_gpu_range(0x1000, 0x2000, 0x1800, 0x100), Some((0x800, 0x100)));
        for (base, length, gpu, bytes) in [
            (0x1000, 0x2000, 0xFFF, 4),
            (0x1000, 0x2000, 0x2FFF, 4),
            (u64::MAX - 3, 8, u64::MAX - 3, 4),
            (0x1000, 0x2000, u64::MAX - 1, 4),
        ] {
            assert!(picasso_vue_gpu_range(base, length, gpu, bytes).is_none());
        }
    }
}
