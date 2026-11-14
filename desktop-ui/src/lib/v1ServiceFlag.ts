/**
 * The backend's compiled `v1-service` value and the frontend's build-time
 * variable must agree.
 *
 * The backend reports whether the feature was compiled; Vite substitutes
 * `VITE_ER_V1_SERVICE` at build time. A disagreement means the two halves were
 * built for different paths, and starting would silently run one while the
 * other is expected.
 */
export class V1ServiceFlagMismatch extends Error {
  constructor(message: string) {
    super(message);
    this.name = "V1ServiceFlagMismatch";
  }
}

export function assertV1ServiceFlagAgreement(
  backend: boolean,
  env: string | undefined,
): void {
  const requested = env === "1";
  if (requested === backend) return;
  throw new V1ServiceFlagMismatch(
    requested
      ? "VITE_ER_V1_SERVICE=1 but the backend build has v1-service off; rebuild the backend with --features v1-service"
      : "the backend build has v1-service on but VITE_ER_V1_SERVICE is not 1; set it (scripts/tauri-dev.sh --v1-service) and rebuild the UI",
  );
}
