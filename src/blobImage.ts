/** Blob으로 그림을 읽고, 읽기가 끝나면(성공·실패 모두) Blob URL을 한 번만 해제한다.
 * 화면이 닫힌 뒤(dispose) 도착한 결과는 버린다. 브라우저 기능은 밖에서 넣어 주므로 노드 테스트에서도 돌린다. */

/** 그림 요소에서 쓰는 부분만 (브라우저에서는 HTMLImageElement). 콜백에 넘어오는 값은 쓰지 않는다 */
type Handler = ((...args: never[]) => unknown) | null;
export interface ImageLike {
  onload: Handler;
  onerror: Handler;
  src: string;
}

export interface BlobImageDeps<I extends ImageLike> {
  createUrl: (blob: Blob) => string;
  revokeUrl: (url: string) => void;
  makeImage: () => I;
}

export interface BlobImageLoader<I extends ImageLike> {
  /** 읽기 시작. 화면이 이미 닫혔으면 아무것도 하지 않는다 */
  load: (blob: Blob, onLoad: (img: I) => void, onFail: () => void) => void;
  /** 화면이 닫힐 때: 아직 읽는 중인 URL을 모두 해제하고, 그 뒤 도착하는 결과는 버린다 */
  dispose: () => void;
  /** 아직 해제하지 않은 URL 수 (테스트용) */
  pending: () => number;
}

export function createBlobImageLoader<I extends ImageLike>(deps: BlobImageDeps<I>): BlobImageLoader<I> {
  // 읽는 중인 것마다 "끝내기" 함수. 끝내기는 URL을 해제하고 다시 불려도 아무 일도 하지 않는다
  const open = new Set<() => void>();
  let closed = false;

  const load = (blob: Blob, onLoad: (img: I) => void, onFail: () => void) => {
    if (closed) return;
    const img = deps.makeImage();
    const url = deps.createUrl(blob);
    let settled = false;
    const settle = () => {
      if (settled) return false;
      settled = true;
      open.delete(settle);
      img.onload = null;
      img.onerror = null;
      deps.revokeUrl(url);
      return true;
    };
    img.onload = () => {
      // 다 읽은 그림은 URL을 해제해도 그대로 그릴 수 있다
      if (settle() && !closed) onLoad(img);
    };
    img.onerror = () => {
      if (settle() && !closed) onFail();
    };
    open.add(settle);
    img.src = url;
  };

  const dispose = () => {
    closed = true;
    for (const settle of [...open]) settle();
  };

  return { load, dispose, pending: () => open.size };
}
