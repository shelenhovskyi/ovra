// How other areas search one folder (the file tree's "Find in folder"): the
// search view, once mounted, handles the request.

type Handler = (include: string) => void | Promise<void>;

let handler: Handler | null = null;

/** Shows the search view limited to the project-relative `folder` (`""`: the whole project), focus in the search box. */
export function findInFolder(folder: string): void {
  void handler?.(folder === "" ? "" : `${folder}/**`);
}

/** Called by the search view with the include glob to set; returns a function that stops handling. */
export function handleFindInFolder(next: Handler): () => void {
  handler = next;
  return () => {
    if (handler === next) handler = null;
  };
}
