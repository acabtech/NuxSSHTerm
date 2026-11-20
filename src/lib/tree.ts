// Pure, immutable helpers for the session tree. No React state here — these are
// shared by the `useSessionTree` hook and any future consumers (search, import preview).

import type { SessionNode } from "../types";

/** Return the node at `path` (array of child indices), or null. */
export function getAt(tree: SessionNode[], path: number[]): SessionNode | null {
  let nodes = tree;
  let node: SessionNode | null = null;
  for (const i of path) {
    node = nodes[i] ?? null;
    if (!node) return null;
    nodes = node.children;
  }
  return node;
}

/** Return a new tree with `patch` merged into the node at `path`. */
export function updateAt(
  tree: SessionNode[],
  path: number[],
  patch: Partial<SessionNode>,
): SessionNode[] {
  if (path.length === 0) return tree;
  const [i, ...rest] = path;
  return tree.map((n, idx) => {
    if (idx !== i) return n;
    return rest.length === 0 ? { ...n, ...patch } : { ...n, children: updateAt(n.children, rest, patch) };
  });
}

/** Return a new tree with `node` inserted under `parentPath` (root when empty). */
export function insertAt(
  tree: SessionNode[],
  parentPath: number[],
  node: SessionNode,
): SessionNode[] {
  if (parentPath.length === 0) return [...tree, node];
  const [i, ...rest] = parentPath;
  return tree.map((n, idx) =>
    idx !== i ? n : { ...n, expanded: true, children: insertAt(n.children, rest, node) },
  );
}

/** Return a new tree with the node at `path` removed. */
export function removeAt(tree: SessionNode[], path: number[]): SessionNode[] {
  if (path.length === 0) return tree;
  const [i, ...rest] = path;
  if (rest.length === 0) return tree.filter((_, idx) => idx !== i);
  return tree.map((n, idx) => (idx !== i ? n : { ...n, children: removeAt(n.children, rest) }));
}

/** Count connection (host) nodes in the tree. */
export function countConnections(tree: SessionNode[]): number {
  let n = 0;
  const walk = (nodes: SessionNode[]) => {
    for (const x of nodes) {
      if (x.type === "Connection") n += 1;
      if (x.children.length) walk(x.children);
    }
  };
  walk(tree);
  return n;
}

/** Map `fn` over every node, preserving structure. */
export function mapAll(
  tree: SessionNode[],
  fn: (n: SessionNode) => SessionNode,
): SessionNode[] {
  return tree.map((n) => fn({ ...n, children: n.children.length ? mapAll(n.children, fn) : [] }));
}

/**
 * Stable vault key for the node at `path`: the ancestor names joined by "/".
 * Used to associate a session's password with its vault entry.
 */
export function pathKey(tree: SessionNode[], path: number[]): string {
  const names: string[] = [];
  let nodes = tree;
  for (const i of path) {
    const n = nodes[i];
    if (!n) return "";
    names.push(n.name);
    nodes = n.children;
  }
  return names.join("/");
}

/**
 * All current vault path keys (one per connection) in the tree. Used to avoid
 * deleting a still-live entry when migrating keys after a rename (issue #32).
 */
export function vaultKeys(tree: SessionNode[]): Set<string> {
  const keys = new Set<string>();
  const walk = (nodes: SessionNode[], prefix: string[]) => {
    for (const n of nodes) {
      const p = [...prefix, n.name];
      if (n.type === "Connection") keys.add(p.join("/"));
      walk(n.children, p);
    }
  };
  walk(tree, []);
  return keys;
}

/**
 * Vault-key migration pairs after a rename (issue #32): walks the pre- and
 * post-update trees in lockstep (an update only patches fields, never
 * structure) and returns (oldKey, newKey) for every connection whose name path
 * changed, so its vault password entry can be moved to the post-rename key.
 * A folder rename yields one pair per descendant connection.
 */
export function passwordKeyMigrations(
  oldTree: SessionNode[],
  newTree: SessionNode[],
): Array<[string, string]> {
  const pairs: Array<[string, string]> = [];
  const walk = (
    a: SessionNode | undefined,
    b: SessionNode,
    oldPrefix: string[],
    newPrefix: string[],
  ) => {
    const oldPath = [...oldPrefix, a?.name ?? b.name];
    const newPath = [...newPrefix, b.name];
    if (b.type === "Connection") {
      const oldKey = oldPath.join("/");
      const newKey = newPath.join("/");
      if (oldKey !== newKey) pairs.push([oldKey, newKey]);
    }
    for (let i = 0; i < b.children.length; i++) {
      walk(a?.children[i], b.children[i], oldPath, newPath);
    }
  };
  for (let i = 0; i < newTree.length; i++) {
    walk(oldTree[i], newTree[i], [], []);
  }
  return pairs;
}

/** Return a new tree with connection passwords filled from a vault map (path key -> password). */
export function applyPasswords(
  tree: SessionNode[],
  map: Record<string, string>,
): SessionNode[] {
  const walk = (nodes: SessionNode[], prefix: string[]): SessionNode[] =>
    nodes.map((n) => {
      const path = [...prefix, n.name];
      const key = path.join("/");
      const next =
        n.type === "Connection" && map[key] ? { ...n, password: map[key] } : n;
      return { ...next, children: n.children.length ? walk(n.children, path) : [] };
    });
  return walk(tree, []);
}

/** Return a new tree with every connection password cleared (used on lock). */
export function clearPasswords(tree: SessionNode[]): SessionNode[] {
  return mapAll(tree, (n) => (n.type === "Connection" ? { ...n, password: "" } : n));
}

/** Generate a unique id (crypto.randomUUID when available). */
export function uid(): string {
  const c = globalThis.crypto as Crypto | undefined;
  if (c && typeof c.randomUUID === "function") return c.randomUUID();
  return `t${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
}