import omigaIconClassicUrl from "./omiga-icon-classic.png";
import omigaLogoClassicUrl from "./omiga-logo.svg";
import omigaLogoWarmUrl from "./omiga-logo-warm.png";
import type { AppSkinId } from "../state/themeStore";

export type AppSkinAsset = {
  id: AppSkinId;
  label: string;
  description: string;
  logoSrc: string;
  windowIconSrc: string;
};

export const APP_SKIN_ASSETS: Record<AppSkinId, AppSkinAsset> = {
  "classic-capybara": {
    id: "classic-capybara",
    label: "Classic Capybara",
    description: "The current calm green Omiga mark.",
    logoSrc: omigaLogoClassicUrl,
    windowIconSrc: omigaIconClassicUrl,
  },
  "warm-capybara": {
    id: "warm-capybara",
    label: "Warm Capybara",
    description: "A brighter candidate based on the new artwork.",
    logoSrc: omigaLogoWarmUrl,
    windowIconSrc: omigaLogoWarmUrl,
  },
};

export function appSkinAssetFor(skin: AppSkinId): AppSkinAsset {
  return APP_SKIN_ASSETS[skin] ?? APP_SKIN_ASSETS["classic-capybara"];
}
