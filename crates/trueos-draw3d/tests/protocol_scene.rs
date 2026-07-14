use trueos_draw3d::{
    ApplyError, ApplyOutcome, Command, Edge, Face, FrameDecoder, Instance, Mesh, Opcode, Response,
    Rgba8, Scene, SceneStats, Transform, Vec3, decode_response, encode_command, encode_response,
};

fn triangle(color: Rgba8) -> Mesh {
    Mesh::new(
        vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ],
        vec![Edge::new(0, 1), Edge::new(1, 2), Edge::new(2, 0)],
        vec![Face::new(vec![0, 1, 2])],
        color,
    )
}

#[test]
fn mesh_and_instance_lifecycle_keeps_references_sane() {
    let mut scene = Scene::default();
    scene
        .apply(Command::PutMesh {
            mesh_id: 10,
            mesh: triangle(Rgba8::new(20, 40, 60, 255)),
        })
        .unwrap();
    scene
        .apply(Command::CopyMesh {
            source_id: 10,
            target_id: 11,
        })
        .unwrap();
    scene
        .apply(Command::PutInstance {
            instance_id: 100,
            instance: Instance::new(10, Transform::IDENTITY),
        })
        .unwrap();

    assert_eq!(
        scene.apply(Command::DeleteMesh {
            mesh_id: 10,
            cascade: false,
        }),
        Err(ApplyError::MeshInUse)
    );
    let outcome = scene
        .apply(Command::DeleteMesh {
            mesh_id: 10,
            cascade: true,
        })
        .unwrap();
    assert_eq!(outcome.affected, 2);
    assert!(scene.instance(100).is_none());
    assert!(scene.mesh(11).is_some());
}

#[test]
fn rejected_vertex_update_is_transactional() {
    let mut scene = Scene::default();
    scene
        .apply(Command::PutMesh {
            mesh_id: 1,
            mesh: triangle(Rgba8::WHITE),
        })
        .unwrap();

    let result = scene.apply(Command::SetVertices {
        mesh_id: 1,
        vertices: vec![Vec3::ZERO],
    });
    assert_eq!(result, Err(ApplyError::VertexIndexOutOfRange));
    assert_eq!(scene.mesh(1).unwrap().vertices.len(), 3);
}

#[test]
fn fragmented_and_coalesced_frames_decode() {
    let first = encode_command(7, &Command::Ping { nonce: 0xfeed_beef }).unwrap();
    let second = encode_command(8, &Command::GetStats).unwrap();
    let split = 5;
    let mut decoder = FrameDecoder::new();
    decoder.push(&first[..split]).unwrap();
    assert!(decoder.next_frame().unwrap().is_none());

    let mut rest = first[split..].to_vec();
    rest.extend_from_slice(&second);
    decoder.push(&rest).unwrap();
    let frame = decoder.next_frame().unwrap().unwrap();
    assert_eq!(frame.request_id, 7);
    assert_eq!(
        trueos_draw3d::protocol::decode_command(frame).unwrap(),
        Command::Ping { nonce: 0xfeed_beef }
    );
    let frame = decoder.next_frame().unwrap().unwrap();
    assert_eq!(frame.request_id, 8);
    assert_eq!(trueos_draw3d::protocol::decode_command(frame).unwrap(), Command::GetStats);
    assert!(decoder.next_frame().unwrap().is_none());
}

#[test]
fn large_mesh_round_trips_without_schema_overhead() {
    const VERTICES: usize = 100_000;
    let vertices = (0..VERTICES)
        .map(|i| Vec3::new(i as f32, (i % 127) as f32, 0.0))
        .collect::<Vec<_>>();
    let faces = (2..VERTICES)
        .step_by(3)
        .map(|i| Face::new(vec![(i - 2) as u32, (i - 1) as u32, i as u32]))
        .collect::<Vec<_>>();
    let command = Command::PutMesh {
        mesh_id: 77,
        mesh: Mesh::new(vertices, Vec::new(), faces, Rgba8::new(3, 4, 5, 255)),
    };
    let bytes = encode_command(99, &command).unwrap();
    assert!(bytes.len() < 1_700_000);

    let mut decoder = FrameDecoder::new();
    for chunk in bytes.chunks(8191) {
        decoder.push(chunk).unwrap();
    }
    let decoded = decoder.next_request().unwrap().unwrap().command.unwrap();
    assert_eq!(decoded, command);
}

#[test]
fn response_frames_round_trip_with_correlation() {
    let response = Response::Applied(ApplyOutcome {
        affected: 3,
        stats: SceneStats {
            mesh_count: 2,
            instance_count: 5,
            vertex_count: 99,
            edge_count: 42,
            face_count: 31,
            mesh_bytes: 2048,
        },
    });
    let bytes = encode_response(Opcode::PutMesh, 1234, response);
    let mut decoder = FrameDecoder::new();
    decoder.push(&bytes).unwrap();
    let frame = decoder.next_frame().unwrap().unwrap();
    assert_eq!(frame.request_id, 1234);
    assert!(frame.is_response);
    assert_eq!(decode_response(frame).unwrap(), response);
}
