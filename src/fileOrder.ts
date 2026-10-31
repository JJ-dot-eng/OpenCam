// 여러 파일을 이어 붙일 순서: 폴더는 보지 않고 파일 이름만, 숫자는 크기대로 (clip2 → clip10).
// node 테스트(tests/file-order.test.mjs)가 바로 불러 쓰므로 다른 화면 코드를 불러오지 않는다.

/** 경로에서 파일 이름만 (Windows "\"와 "/" 둘 다) */
export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/** 파일 이름 순서. 대소문자는 가리지 않고, 이름이 같으면 전체 경로로 정해 늘 같은 순서가 되게 한다. */
export function compareFileNames(a: string, b: string): number {
  const opt: Intl.CollatorOptions = { numeric: true, sensitivity: "base" };
  return baseName(a).localeCompare(baseName(b), undefined, opt) || a.localeCompare(b, undefined, { numeric: true });
}

/** 파일 이름 순서로 정렬한 새 목록 */
export function sortByFileName(paths: string[]): string[] {
  return [...paths].sort(compareFileNames);
}
