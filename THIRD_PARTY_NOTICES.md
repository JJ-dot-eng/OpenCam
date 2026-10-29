# 함께 쓰는 부품과 라이선스 (Third-party notices)

OpenCam은 GPL-3.0-or-later로 배포합니다 ([LICENSE](LICENSE)). 아래 부품을 함께 씁니다.

| 부품 | 쓰는 곳 | 라이선스 |
|---|---|---|
| [libimagequant](https://github.com/ImageOptim/libimagequant) (`imagequant` crate) | GIF·APNG 256색 줄이기 | GPL-3.0-or-later |
| [TF-ESPCN](https://github.com/fannymonori/TF-ESPCN) 학습 가중치 | 고품질 확대(초해상도) | Apache-2.0 ([LICENSE-ESPCN.txt](src-tauri/models/LICENSE-ESPCN.txt)) |
| [Tabler Icons](https://github.com/tabler/tabler-icons) (`@tabler/icons-react`) | 화면 아이콘 | MIT, Copyright (c) 2020-2024 Paweł Kuna |
| [Tauri](https://tauri.app) | 창·웹뷰 | MIT 또는 Apache-2.0 |
| [React](https://react.dev) | 화면 | MIT, Copyright (c) Meta Platforms, Inc. and affiliates |
| [libwebp](https://chromium.googlesource.com/webm/libwebp) (`libwebp-sys2`, `webp-animation`) | WebP 저장 | BSD-3-Clause (Copyright (c) 2010, Google Inc.) / MIT 또는 Apache-2.0 |
| [image](https://github.com/image-rs/image), [gif](https://github.com/image-rs/image-gif), [png](https://github.com/image-rs/image-png) | 그림 읽기·쓰기 | MIT 또는 Apache-2.0 |
| [fast_image_resize](https://github.com/Cykooz/fast_image_resize) | 크기 바꾸기 | MIT 또는 Apache-2.0 |
| [windows](https://github.com/microsoft/windows-rs) | 화면 캡처·창 다루기 | MIT 또는 Apache-2.0 |

그 밖의 Rust·npm 부품은 `src-tauri/Cargo.lock`, `package-lock.json`에 적힌 버전을 씁니다. MIT·Apache-2.0이 대부분이고
BSD·Zlib·BSL-1.0·Unicode-3.0·MPL-2.0 부품도 있으며, 모두 GPL-3.0과 함께 배포할 수 있는 라이선스입니다.

프로그램에 들어가는 모든 부품의 저작권 표시와 라이선스 전문은 [THIRD_PARTY_LICENSES.txt](THIRD_PARTY_LICENSES.txt)에
모아 두었고, 설치 파일과 포터블 zip에도 함께 들어갑니다. 부품을 바꾸면 `node scripts/third-party-licenses.mjs`로 다시 만듭니다.

MP4·WebM·AVIF 저장에 쓰는 **ffmpeg는 OpenCam에 들어 있지 않습니다.** 사용자가 따로 설치한 ffmpeg.exe를 불러 씁니다.
