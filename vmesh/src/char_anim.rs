use std::fs::File;
use std::io::BufWriter;
use std::vec::Vec;
use std::f32;
use std::path::Path;
use crate::{rfa, v3mc, gltf_to_rf_quat, gltf_to_rf_vec, quat_to_array};
use crate::import::BufferData;
use crate::io_utils::new_custom_error;

fn gltf_time_to_rfa_time(time_sec: f32) -> i32 {
    (time_sec * 30.0f32 * 160.0f32) as i32
}

fn make_short_quat(quat: [f32; 4]) -> [i16; 4] {
    quat.map(|x| (x * 16383.0f32) as i16)
}

fn get_node_anim_channels<'a>(n: &gltf::Node, anim: &'a gltf::Animation) -> impl Iterator<Item = gltf::animation::Channel<'a>> + 'a {
    let node_index = n.index();
    anim.channels()
        .filter(move |c| c.target().node().index() == node_index)
}

fn convert_rotation_keys(n: &gltf::Node, anim: &gltf::Animation, buffers: &[BufferData]) -> Vec<rfa::RotationKey> {
    get_node_anim_channels(n, anim)
        .filter_map(|channel| {
            let reader = channel.reader(|buffer| Some(&buffers[buffer.index()]));
            let interpolation = channel.sampler().interpolation();
            reader.read_inputs()
                .map(|inputs| reader.read_outputs().map(|outputs| (inputs, outputs, interpolation)))
                .flatten()
        })
        .filter_map(|(inputs, outputs, interpolation)| {
            use gltf::animation::util::ReadOutputs;
            match outputs {
                ReadOutputs::Rotations(rotations) => Some((inputs, rotations, interpolation)),
                _ => None,
            }
        })
        .map(|(inputs, rotations, interpolation)| {
            use gltf::animation::Interpolation;
            let rotations_quads = rotations
                .into_f32()
                .map(|r| make_short_quat(gltf_to_rf_quat(r)));
            let chunked_rotations = if interpolation == Interpolation::CubicSpline {
                rotations_quads
                    .collect::<Vec<_>>()
                    .chunks(3)
                    .map(|s| (s[0], s[1], s[2]))
                    .collect::<Vec<_>>()
            } else {
                rotations_quads
                    .map(|r| (r, r, r))
                    .collect::<Vec<_>>()
            };
            inputs
                .map(gltf_time_to_rfa_time)
                .zip(chunked_rotations)
                .map(|(time, (_, rotation, _))| rfa::RotationKey {
                    time,
                    rotation,
                    ease_in: 0,
                    ease_out: 0,
                })
                .collect::<Vec<_>>()
        })
        .next()
        .unwrap_or_default()
}

fn convert_translation_keys(n: &gltf::Node, anim: &gltf::Animation, buffers: &[BufferData]) -> Vec<rfa::TranslationKey> {
    get_node_anim_channels(n, anim)
        .filter_map(|channel| {
            let reader = channel.reader(|buffer| Some(&buffers[buffer.index()]));
            let interpolation = channel.sampler().interpolation();
            reader.read_inputs()
                .map(|inputs| reader.read_outputs().map(|outputs| (inputs, outputs, interpolation)))
                .flatten()
        })
        .filter_map(|(inputs, outputs, interpolation)| {
            use gltf::animation::util::ReadOutputs;
            match outputs {
                ReadOutputs::Translations(translations) => Some((inputs, translations, interpolation)),
                _ => None,
            }
        })
        .map(|(inputs, translations, interpolation)| {
            use gltf::animation::Interpolation;
            let rf_translations = translations.map(gltf_to_rf_vec);
            let chunked_translations = if interpolation == Interpolation::CubicSpline {
                rf_translations
                    .collect::<Vec<_>>()
                    .chunks(3)
                    .map(|s| (s[0], s[1], s[2]))
                    .collect::<Vec<_>>()
            } else {
                rf_translations
                    .map(|t| (t, t, t))
                    .collect::<Vec<_>>()
            };
            inputs
                .map(gltf_time_to_rfa_time)
                .zip(chunked_translations)
                .map(|(time, (_, translation, _))|
                    // ignore cubic spline tangents for now - RF uses bezier curve and tangents are different
                    rfa::TranslationKey {
                        time,
                        in_tangent: translation,
                        translation,
                        out_tangent: translation,
                    }
                )
                .collect::<Vec<_>>()
        })
        .next()
        .unwrap_or_default()
}

fn determine_anim_time_range(bones: &[rfa::Bone]) -> (i32, i32) {
    bones.iter()
        .flat_map(|b| b.rotation_keys.iter()
            .map(|k| k.time)
            .chain(b.translation_keys.iter().map(|k| k.time)))
        .fold((0i32, 0i32), |(min, max), time| (min.min(time), max.max(time)))
}

fn make_rfa(anim: &gltf::Animation, skin: &gltf::Skin, buffers: &[BufferData]) -> rfa::File {
    let mut bones = Vec::with_capacity(skin.joints().count());
    for n in skin.joints() {
        let rotation_keys = convert_rotation_keys(&n, anim, buffers);
        let translation_keys = convert_translation_keys(&n, anim, buffers);
        bones.push(rfa::Bone {
            weight: 1.0f32,
            rotation_keys,
            translation_keys,
        });
    }
    let (start_time, end_time) = determine_anim_time_range(&bones);
    let header = rfa::FileHeader {
        num_bones: bones.len() as i32,
        start_time,
        end_time,
        ramp_in_time: 480,
        ramp_out_time: 480,
        total_rotation: [0.0f32, 0.0f32, 0.0f32, 1.0f32],
        total_translation: [0.0f32, 0.0f32, 0.0f32],
        ..rfa::FileHeader::default()
    };
    rfa::File {
        header,
        bones,
    }
}

pub(crate) fn convert_animation_to_rfa(anim: &gltf::Animation, index: usize, skin: &gltf::Skin, buffers: &[BufferData], output_dir: &Path) -> std::io::Result<()> {
    let name = anim.name().map(&str::to_owned).unwrap_or_else(|| format!("anim_{}", index));
    println!("Processing animation {}", name);
    let file_name = output_dir.join(format!("{}.rfa", name));
    let mut wrt = BufWriter::new(File::create(file_name)?);
    let rfa = make_rfa(anim, skin, buffers);
    rfa.write(&mut wrt)?;
    Ok(())
}

fn get_joint_index(node: &gltf::Node, skin: &gltf::Skin) -> usize {
    skin.joints().enumerate()
        .filter(|(_i, n)| node.index() == n.index())
        .map(|(i, _n)| i)
        .next()
        .expect("joint not found")
}

fn get_joint_parent<'a>(node: &gltf::Node, skin: &gltf::Skin<'a>) -> Option<gltf::Node<'a>> {
    skin.joints().find(|n| n.children().any(|c| c.index() == node.index()))
}

fn convert_bone(n: &gltf::Node, inverse_bind_matrix: &[[f32; 4]; 4], index: usize, skin: &gltf::Skin) -> v3mc::Bone {
    let name = n.name().map(&str::to_owned).unwrap_or_else(|| format!("bone_{}", index));
    let parent_node_opt = get_joint_parent(n, skin);
    let parent_index = parent_node_opt
        .map(|pn| get_joint_index(&pn, skin) as i32)
        .unwrap_or(-1);
    let inv_transform = glam::Mat4::from_cols_array_2d(inverse_bind_matrix);
    let (gltf_scale, gltf_rotation, gltf_translation) = inv_transform.to_scale_rotation_translation();
    assert!((gltf_scale - glam::Vec3::ONE).max_element() < 0.01f32, "scale is not supported: {}", gltf_scale);
    let base_rotation = gltf_to_rf_quat(quat_to_array(&gltf_rotation));
    let base_translation = gltf_to_rf_vec(gltf_translation.to_array());
    v3mc::Bone { name, base_rotation, base_translation, parent_index }
}

pub(crate) fn convert_bones(skin: &gltf::Skin, buffers: &[BufferData]) -> std::io::Result<Vec<v3mc::Bone>> {
    let num_joints = skin.joints().count();
    if num_joints > v3mc::MAX_BONES {
        let err_msg = format!("too many bones: found {} but only {} are supported", num_joints, v3mc::MAX_BONES);
        return Err(new_custom_error(err_msg));
    }

    let inverse_bind_matrices: Vec<_> = skin.reader(|buffer| Some(&buffers[buffer.index()]))
        .read_inverse_bind_matrices()
        .expect("expected inverse bind matrices")
        .collect();

    if inverse_bind_matrices.len() != num_joints {
        let err_msg = format!("invalid number of inverse bind matrices: expected {}, got {}",
            num_joints, inverse_bind_matrices.len());
        return Err(new_custom_error(err_msg));
    }

    let mut bones = Vec::with_capacity(num_joints);
    for (i, n) in skin.joints().enumerate() {
        let bone = convert_bone(&n, &inverse_bind_matrices[i], i, skin);
        bones.push(bone);
    }
    Ok(bones)
}
