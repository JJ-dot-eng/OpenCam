# 초해상도 모델 가중치

`espcn_x2.bin`, `espcn_x3.bin`, `espcn_x4.bin`은 [fannymonori/TF-ESPCN](https://github.com/fannymonori/TF-ESPCN)의
`export/ESPCN_x2.pb`, `ESPCN_x3.pb`, `ESPCN_x4.pb`(Apache License 2.0, `LICENSE-ESPCN.txt`)에서 가중치만 꺼낸 것이다.
OpenCV dnn_superres 모델의 원본이다. 가중치 값은 바꾸지 않았다.

형식: 32비트 float(리틀 엔디언)을 `f1 b1 f2 b2 f3 b3` 순서로 이어 붙였다.
합성곱 가중치는 TensorFlow 순서 `[높이][너비][입력 채널][출력 채널]`이다.

| 층 | 모양 |
|---|---|
| f1, b1 | 5×5×1×64, 64 (ReLU) |
| f2, b2 | 3×3×64×32, 32 (ReLU) |
| f3, b3 | 3×3×32×r², r² → 픽셀 재배치(depth_to_space) → tanh |

입력은 밝기(Y, 0~1, OpenCV YCrCb와 같은 식)이고 출력은 r배 커진 밝기다. 색(Cr·Cb)은 따로 부드럽게 늘린다.
