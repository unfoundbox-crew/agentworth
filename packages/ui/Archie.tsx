import React from "react";

import frontSit from "./brand/archie/front-sit.svg?raw";
import threeQuarter from "./brand/archie/three-quarter.svg?raw";
import sideScent from "./brand/archie/side-scent.svg?raw";
import digging from "./brand/archie/digging.svg?raw";
import fetching from "./brand/archie/fetching.svg?raw";
import dropping from "./brand/archie/dropping.svg?raw";
import sleeping from "./brand/archie/sleeping.svg?raw";
import errorPose from "./brand/archie/error.svg?raw";

export type ArchiePose =
  | "front-sit"
  | "three-quarter"
  | "side-scent"
  | "digging"
  | "fetching"
  | "dropping"
  | "sleeping"
  | "error";

export type ArchieAccessory = "lamp" | "goggles" | "none";
export type ArchieColourway = "C1" | "C2" | "C3" | "C4";
export type ArchieMotion = "dig" | "run" | "found" | "error";

export const ARCHIE_ACCESSORIES: ArchieAccessory[] = ["lamp", "goggles", "none"];
export const ARCHIE_COLOURWAYS: ArchieColourway[] = ["C1", "C2", "C3", "C4"];

/** Bare (Saurabh, 2026-09-02). The head gear is a switch, not how he arrives. */
export const ARCHIE_DEFAULT_ACCESSORY: ArchieAccessory = "none";
export const ARCHIE_DEFAULT_COLOURWAY: ArchieColourway = "C3";

const SOURCES: Record<ArchiePose, string> = {
  "front-sit": frontSit,
  "three-quarter": threeQuarter,
  "side-scent": sideScent,
  digging,
  fetching,
  dropping,
  sleeping,
  error: errorPose,
};

export function isAccessory(value: unknown): value is ArchieAccessory {
  return typeof value === "string" && (ARCHIE_ACCESSORIES as string[]).includes(value);
}

export function isColourway(value: unknown): value is ArchieColourway {
  return typeof value === "string" && (ARCHIE_COLOURWAYS as string[]).includes(value);
}

export interface ArchieProps {
  pose: ArchiePose;
  size?: number;
  accessory?: ArchieAccessory;
  colourway?: ArchieColourway;
  motion?: ArchieMotion;
  className?: string;
  /** Overrides the pose file's own aria-label. Pass "" for a decorative figure. */
  label?: string;
}

/**
 * One Archie pose, inlined rather than sourced through an `<img>`.
 *
 * The colourways are CSS custom properties, and a document's custom properties do not
 * reach inside an `<img>` — so an image would render Archie in the fallback palette on
 * every surface. The markup is patched rather than re-authored: the SVG files in
 * `brand/archie/` stay the only place the drawing exists.
 */
export const Archie: React.FC<ArchieProps> = ({
  pose,
  size = 160,
  accessory = ARCHIE_DEFAULT_ACCESSORY,
  colourway = ARCHIE_DEFAULT_COLOURWAY,
  motion,
  className = "",
  label,
}) => {
  const markup = React.useMemo(() => {
    let svg = SOURCES[pose]
      .replace(/\swidth="[^"]*"/, ` width="${size}"`)
      .replace(/\sheight="[^"]*"/, ` height="${size}"`)
      .replace(/\sdata-accessory="[^"]*"/, ` data-accessory="${accessory}"`);
    if (label !== undefined) {
      svg = label
        ? svg.replace(/\saria-label="[^"]*"/, ` aria-label="${label}"`)
        : svg.replace(/\srole="img"/, ' aria-hidden="true"').replace(/\saria-label="[^"]*"/, "");
    }
    return svg;
  }, [pose, size, accessory, label]);

  const classes = [
    "archie-figure",
    `archie-c${colourway.slice(1)}`,
    motion ? `archie-${motion}` : "",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return <span className={classes} dangerouslySetInnerHTML={{ __html: markup }} />;
};
