export type SettledTask<T> =
  | { ok: true; value: T }
  | { ok: false; error: string };

export async function settleTask<T>(task: Promise<T>): Promise<SettledTask<T>> {
  try {
    return { ok: true, value: await task };
  } catch (error) {
    return { ok: false, error: error instanceof Error ? error.message : String(error) };
  }
}
