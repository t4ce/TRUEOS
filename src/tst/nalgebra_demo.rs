use nalgebra::linalg::{Cholesky, QR, SVD};
use nalgebra::{Matrix3, Matrix4, Point3, Rotation3, Translation3, Vector3};

#[embassy_executor::task]
pub(crate) async fn boot_nalgebra_demo_task() {
    async move {
    // crate::log!("nalgebra-demo: starting\n");

    // Vector ops
    let v = Vector3::new(1.0f32, 2.0, 3.0);
    let w = Vector3::new(-2.0f32, 0.5, 4.0);
    let _dot = v.dot(&w);
    let _cross = v.cross(&w);
    let _v_norm = v.normalize();
    // crate::log!(
    //     "nalgebra-demo: vec: v={:?} w={:?} dot={} cross={:?} norm_v={:?}\n",
    //     v,
    //     w,
    //     dot,
    //     cross,
    //     v_norm
    // );

    // Matrix ops
    let m = Matrix3::new(1.0, 2.0, 3.0, 0.0, 1.0, 4.0, 5.0, 6.0, 0.0);
    let mt = m.transpose();
    let _mm = m * mt;
    // crate::log!("nalgebra-demo: mat3: m={:?} mt={:?} mmt={:?}\n", m, mt, mm);
    match m.try_inverse() {
        Some(_inv) => {
            // crate::log!("nalgebra-demo: mat3 inv={:?}\n", inv)
        }
        None => {
            // crate::log!("nalgebra-demo: mat3 inv=none\n")
        }
    }

    // Matrix4 ops
    let a4 = Matrix4::new(
        1.0, 0.0, 0.0, 10.0, 0.0, 0.0, -1.0, -2.0, 0.0, 1.0, 0.0, 1.5, 0.0, 0.0, 0.0, 1.0,
    );
    let b4 = Matrix4::new(
        0.5, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 1.0,
    );
    let _c4 = a4 * b4;
    // crate::log!("nalgebra-demo: mat4: a={:?} b={:?} a*b={:?}\n", a4, b4, c4);

    // Transformations
    let rot = Rotation3::from_euler_angles(0.3, -0.2, 0.5);
    let trans = Translation3::new(10.0, -2.0, 1.5);
    let p = Point3::new(1.0, 2.0, 3.0);
    let p_rot = rot.transform_point(&p);
    let _p_xform = trans.transform_point(&p_rot);
    // crate::log!(
    //     "nalgebra-demo: xform: p={:?} rot_p={:?} trans_rot_p={:?}\n",
    //     p,
    //     p_rot,
    //     p_xform
    // );

    // Decompositions
    let m_svd = Matrix3::new(1.0, 2.0, 3.0, 0.0, 1.0, 4.0, 5.0, 6.0, 0.0);
    let svd = SVD::new(m_svd, true, true);
    // crate::log!(
    //     "nalgebra-demo: svd: sigma={:?} u?={} vt?={}\n",
    //     svd.singular_values,
    //     svd.u.is_some(),
    //     svd.v_t.is_some()
    // );
    if let (Some(u), Some(vt)) = (svd.u, svd.v_t) {
        let s = Matrix3::from_diagonal(&svd.singular_values);
        let _recon = u * s * vt;
        // crate::log!("nalgebra-demo: svd recon={:?}\n", recon);
    }

    let m_qr = Matrix3::new(1.0, 2.0, 3.0, 0.0, 1.0, 4.0, 5.0, 6.0, 0.0);
    let qr = QR::new(m_qr);
    let _q = qr.q();
    let _r = qr.r();
    // crate::log!("nalgebra-demo: qr: q={:?} r={:?}\n", q, r);

    let a_chol = Matrix3::new(4.0, 1.0, 1.0, 1.0, 3.0, 0.0, 1.0, 0.0, 2.0);
    match Cholesky::new(a_chol) {
        Some(chol) => {
            let _l = chol.l();
            // crate::log!("nalgebra-demo: cholesky: l={:?}\n", l);
        }
        None => {
            // crate::log!("nalgebra-demo: cholesky: none\n")
        }
    }

    crate::log!("nalgebra-demo: done\n");
    }.await;
}