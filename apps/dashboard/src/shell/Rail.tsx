import type { ComponentType } from 'react';
import { IconDock, IconMeter, IconCargo, IconProbe, IconDownload } from './dsIcons';
import type { DsIconProps } from './dsIcons';

export type RailViewId = 'sessions' | 'overview' | 'coverage' | 'archaeology' | 'exports';

interface RailItem {
  id: RailViewId;
  label: string;
  Icon: ComponentType<DsIconProps>;
}

// Mapping onto the design-system sprite (24x24, square caps, miter joins):
//   sessions    -> i-dock      stacked rows read as a list
//   overview    -> i-meter     a gauge
//   coverage    -> i-cargo     2x2 grid renders as a coverage matrix
//   archaeology -> i-probe     digging into a trace
//   exports     -> i-download
const ITEMS: RailItem[] = [
  { id: 'sessions', label: 'Sessions', Icon: IconDock },
  { id: 'overview', label: 'Overview', Icon: IconMeter },
  { id: 'coverage', label: 'Coverage', Icon: IconCargo },
  { id: 'archaeology', label: 'Archaeology', Icon: IconProbe },
  { id: 'exports', label: 'Exports', Icon: IconDownload },
];

export interface RailProps {
  activeView: RailViewId;
  onSelect: (view: RailViewId) => void;
}

export function Rail({ activeView, onSelect }: RailProps) {
  return (
    <nav className="rail" aria-label="Sections">
      {ITEMS.map((item) => (
        <button
          key={item.id}
          type="button"
          className="rail-btn"
          data-tooltip={item.label}
          aria-current={item.id === activeView ? 'true' : undefined}
          aria-label={item.label}
          title={item.label}
          onClick={() => onSelect(item.id)}
        >
          <item.Icon size={20} />
        </button>
      ))}
      <div className="rail-spacer" />
    </nav>
  );
}
