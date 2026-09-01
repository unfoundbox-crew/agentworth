export type RailViewId = 'sessions' | 'overview' | 'coverage' | 'archaeology' | 'exports';

interface RailItem {
  id: RailViewId;
  label: string;
  path: JSX.Element;
}

const ITEMS: RailItem[] = [
  {
    id: 'sessions',
    label: 'Sessions',
    path: (
      <>
        <circle cx="4" cy="6" r="1" />
        <line x1="8" y1="6" x2="17" y2="6" />
        <circle cx="4" cy="10" r="1" />
        <line x1="8" y1="10" x2="17" y2="10" />
        <circle cx="4" cy="14" r="1" />
        <line x1="8" y1="14" x2="17" y2="14" />
      </>
    ),
  },
  {
    id: 'overview',
    label: 'Overview',
    path: (
      <>
        <line x1="4.5" y1="16" x2="4.5" y2="11" />
        <line x1="10" y1="16" x2="10" y2="6" />
        <line x1="15.5" y1="16" x2="15.5" y2="9" />
      </>
    ),
  },
  {
    id: 'coverage',
    label: 'Coverage',
    path: (
      <>
        <circle cx="10" cy="10" r="7" />
        <polyline points="6.5 10 9 12.5 13.5 7.5" />
      </>
    ),
  },
  {
    id: 'archaeology',
    label: 'Archaeology',
    path: (
      <>
        <path d="M10 3 L17 7 L10 11 L3 7 Z" />
        <polyline points="3 10.5 10 14.5 17 10.5" />
        <polyline points="3 13.5 10 17.5 17 13.5" />
      </>
    ),
  },
  {
    id: 'exports',
    label: 'Exports',
    path: (
      <>
        <line x1="10" y1="3" x2="10" y2="12.5" />
        <polyline points="6.5 9.5 10 13 13.5 9.5" />
        <polyline points="4 14.5 4 17 16 17 16 14.5" />
      </>
    ),
  },
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
          <svg
            viewBox="0 0 20 20"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            {item.path}
          </svg>
        </button>
      ))}
      <div className="rail-spacer" />
    </nav>
  );
}
