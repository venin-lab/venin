/** Cuts of the 4K render, each named by its width: exact for the header at 1x, 2x and 3x and for the card. */
const CUTS = [32, 48, 64, 96, 128, 256, 512, 1024];
const SRCSET = CUTS.map((w) => `/brand/venin-mark-3d-${w}.webp ${w}w`).join(", ");

/** The titanium mark, the 4K render cut out on a transparent ground, at `size` CSS pixels. */
export function Mark({ size, class: cls }: { size: number; class?: string }) {
  return (
    <img
      class={cls}
      src="/brand/venin-mark-3d-128.webp"
      srcset={SRCSET}
      sizes={`${size}px`}
      width={size}
      height={size}
      alt=""
    />
  );
}
