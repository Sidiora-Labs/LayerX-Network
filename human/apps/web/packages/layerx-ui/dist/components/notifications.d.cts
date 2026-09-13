import * as React from 'react';
import { NotificationItem, RecencySegment } from '../lib/types.cjs';

declare function NotificationsScreen({ items, onBack, onItemClick, segment: segmentProp, onSegmentChange, className, }: {
    items: NotificationItem[];
    onBack?: () => void;
    onItemClick?: (item: NotificationItem) => void;
    segment?: RecencySegment;
    onSegmentChange?: (s: RecencySegment) => void;
    className?: string;
}): React.JSX.Element;
declare function BellPopover({ items, onItemClick, onViewAll, unreadCount, }: {
    items: NotificationItem[];
    onItemClick?: (item: NotificationItem) => void;
    onViewAll?: () => void;
    unreadCount?: number;
}): React.JSX.Element;
/** Full archive page (desktop counterpart to the mobile pushed screen). */
declare function NotificationsArchive({ items, onItemClick, segment: segmentProp, onSegmentChange, className, }: {
    items: NotificationItem[];
    onItemClick?: (item: NotificationItem) => void;
    segment?: RecencySegment;
    onSegmentChange?: (s: RecencySegment) => void;
    className?: string;
}): React.JSX.Element;

export { BellPopover, NotificationsArchive, NotificationsScreen };
