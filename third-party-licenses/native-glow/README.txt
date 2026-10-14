Native Glow smoke dependency and license review - 2026-10-14

No Cargo dependency, feature, profile, lockfile or toolchain change.
The standalone test linker promotes the following existing pinned transitive
artifacts to explicit probe inputs. They are not new Workbench dependencies.
glutin/glutin-winit/winit/raw-window-handle expose native window/context lifetime;
Glow comes from the pinned eframe reexport and supplies checked raw GL commands.
No egui_glow Painter implementation or shaders are copied into the adapter.
The adapter shaders are independently written for the fixed egui vertex contract.

Safety review: Windows event-loop thread only, unshared GL 3.3 context, no
Runtime/client connection, bounded staging/buffers, one namespace, explicit
fence and healthy shutdown. Unsafe is confined to the standalone device module.
Raw handles remain private. Terminal device-loss/destructor qualification is open.
This is a source/license review for the test harness, not a security certification.
Existing transitive resolution and published release license evidence are intact.
Glow licenses retain their exact upstream CRLF bytes; the narrow .gitattributes
whitespace rule recognizes CR at line endings without normalizing those files.

glow 0.17.0 | MIT OR Apache-2.0 OR Zlib
  Cargo archive SHA256: 29038e1c483364cc6bb3cf78feee1816002e127c331a1eec55a4d202b9e1adb5
  third-party-licenses/native-glow/glow-0.17.0/LICENSE-APACHE SHA256 b8601fc79bdf8d8088f0938bea861ffc63ed0eebbedf25f44ab2cf2e85bd7edc
  third-party-licenses/native-glow/glow-0.17.0/LICENSE-MIT SHA256 cdbd06e25b8c9c5d6019949d2de8123f56d29cee4166396423d2ba8c81700845
  third-party-licenses/native-glow/glow-0.17.0/LICENSE-ZLIB SHA256 636f644f1e149eb62961657c0e34953681eff64435653912e7bd19992666faa5

glutin 0.32.3 | Apache-2.0
  Cargo archive SHA256: 12124de845cacfebedff80e877bb37b5b75c34c5a4c89e47e1cdd67fb6041325
  third-party-licenses/native-glow/glutin-0.32.3/LICENSE SHA256 a4445427440043e8ce6fadf7a43fb906cb0d7f6c573a560a9f142cf9f9bf8ff4

glutin-winit 0.5.0 | MIT
  Cargo archive SHA256: 85edca7075f8fc728f28cb8fbb111a96c3b89e930574369e3e9c27eb75d3788f
  third-party-licenses/native-glow/glutin-winit-0.5.0/LICENSE SHA256 d35629aec828ca86834f76e4c813bd6a6f0dda9005da898b0f49021f8042c5bd

winit 0.30.13 | Apache-2.0
  Cargo archive SHA256: a6755fa58a9f8350bd1e472d4c3fcc25f824ec358933bba33306d0b63df5978d
  third-party-licenses/native-glow/winit-0.30.13/LICENSE SHA256 6dc0e068dcf3a5bc8e054205b85b7720e1d49265bbc64bf515d2cf79197df69a

raw-window-handle 0.6.2 | MIT OR Apache-2.0 OR Zlib
  Cargo archive SHA256: 20675572f6f24e9e76ef639bc5552774ed45f1c30e2951e1e99c59888861c539
  third-party-licenses/native-glow/raw-window-handle-0.6.2/LICENSE-APACHE.md SHA256 0d542e0c8804e39aa7f37eb00da5a762149dc682d7829451287e11b938e94594
  third-party-licenses/native-glow/raw-window-handle-0.6.2/LICENSE-MIT.md SHA256 9c5a80639a57c1c945570e7ebbca0706305849ce3c098021325cca9db2f7acc4
  third-party-licenses/native-glow/raw-window-handle-0.6.2/LICENSE-ZLIB.md SHA256 ae895c8576e682f310fb70d93d9f99b1dd82872094c54a8f6cdfe2ca5c2d6ecb

Pinned source audit SHA256 (local Cargo registry relative paths):
egui_glow-0.36.2/src/painter.rs fbebbfb983c2100993a2351a8513c675fe3ec6a099067993a192bedc5030140d
egui_glow-0.36.2/src/lib.rs 74ebbb81b9e0edf8b1e1f76b1f8b8cfe1cfc1d5fb8b3a4cd4393dc7b6483f839
glow-0.17.0/src/native.rs 48b3d02cc462b8043ab361338befeaf53a7dff42309aee5701bf943855cf2d0f
glow-0.17.0/src/lib.rs 5ccfa4e62fc01bb10a2896ff8a7403d8c1250fb48beb84c02e5c9888728c0844
glutin-0.32.3/src/api/wgl/context.rs 280d68e2e3fb4efa9c9396309237b9af2baff970abc4c4cbd68679c3092831f0
glutin-0.32.3/src/context.rs 4d199dc49a029633fa64c2df0fe9f21c0d40bc6ea4a432474bd4e0444a479803
glutin-winit-0.5.0/src/lib.rs cd2bf6b77e869d4c08a17888a398353a6c65cf690cf0828eb3d101fe0abffbe4
winit-0.30.13/src/application.rs 993ddc2bbffca969224b2045dfd6cefd6cadd923df83f8ba24da8da8e5ab3fb4
winit-0.30.13/src/platform/windows.rs 1a480966b317ac2f3117b26d379954b2d9af1740472a9ae07e055598862a24eb
