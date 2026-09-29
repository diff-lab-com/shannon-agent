import { clsx, type ClassValue } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

// Batch 1 moved the type scale onto custom roles (text-body-sm, text-label-md,
// …). tailwind-merge's default config doesn't know those names, so it filed
// them in the SAME group as text colors — "text-on-primary" and "text-body-sm"
// in one string silently deleted each other and primary buttons rendered with
// inherited text (caught as a serious axe color-contrast violation on /memory
// in the G7 walkthrough). Pin the roles to the font-size group so size and
// color classes compose instead of conflict.
const twMerge = extendTailwindMerge({
  extend: {
    classGroups: {
      "font-size": [
        {
          text: [
            "label-2xs",
            "label-xs",
            "label-sm",
            "label-md",
            "body-sm",
            "body-md",
            "body-lg",
            "headline-sm",
            "headline-md",
            "headline-lg",
            "display-lg",
          ],
        },
      ],
    },
  },
});

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
