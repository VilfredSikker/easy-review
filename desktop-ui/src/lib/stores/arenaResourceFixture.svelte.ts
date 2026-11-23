/** A `jobs` version the arena resource's test moves by hand, as a rune. */
let version = $state<number | null>(null);

export const jobsFixture = {
  jobsVersion: (): number | null => version,
  move: (): void => {
    version = (version ?? 0) + 1;
  },
};
