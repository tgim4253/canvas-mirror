import type {
  ManagedRoomDto,
  ManagedRoomViewerLinkDto,
} from "../../../shared/type";
import type { RoomCardProps } from "../ui";

export type RoomCardView = ManagedRoomDto;
export type RoomViewerLinkView = ManagedRoomViewerLinkDto & {
  index: number;
};

export function getPrimaryViewerLink(
  view: RoomCardView,
): RoomViewerLinkView | null {
  return getVisibleViewerLinks(view)[0] ?? null;
}

export function getPrimaryViewerUrl(view: RoomCardView): string | null {
  return getPrimaryViewerLink(view)?.viewer_url ?? null;
}

export function getVisibleViewerLinks(
  view: RoomCardView,
): RoomViewerLinkView[] {
  return view.viewer_links.flatMap((link, index) =>
    shouldDisplayViewerUrl(link.viewer_url) ? [{ ...link, index }] : [],
  );
}

export function toRoomCardProps(view: RoomCardView): RoomCardProps {
  const viewerLinks = getVisibleViewerLinks(view);

  return {
    name: view.room.room.name,
    sourcePath: view.target_path,
    previewUrl: view.preview_data_url ?? undefined,
    viewerLinks: viewerLinks.map((link) => ({
      hasQr: Boolean(link.qr_svg),
      viewerUrl: link.viewer_url,
      sourceIndex: link.index,
    })),
    deviceCount: view.room.devices.length,
    status: view.room.state,
  };
}

function shouldDisplayViewerUrl(viewerUrl: string): boolean {
  try {
    const { hostname } = new URL(viewerUrl);
    return !hostname.startsWith("127.");
  } catch {
    return true;
  }
}
