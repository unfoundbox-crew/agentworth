interface RailItem {
  id: string;
  label: string;
  active: boolean;
  path: JSX.Element;
}

const ITEMS: RailItem[] = [
  {
    id: 'sessions',
    label: 'Sessions',
    active: true,
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
    id: 'coverage',
    label: 'Coverage',
    active: false,
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
    active: false,
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
    active: false,
    path: (
      <>
        <line x1="10" y1="3" x2="10" y2="12.5" />
        <polyline points="6.5 9.5 10 13 13.5 9.5" />
        <polyline points="4 14.5 4 17 16 17 16 14.5" />
      </>
    ),
  },
  {
    id: 'settings',
    label: 'Settings',
    active: false,
    path: (
      <>
        <line x1="3" y1="6" x2="17" y2="6" />
        <circle cx="8" cy="6" r="1.8" />
        <line x1="3" y1="10" x2="17" y2="10" />
        <circle cx="13" cy="10" r="1.8" />
        <line x1="3" y1="14" x2="17" y2="14" />
        <circle cx="6" cy="14" r="1.8" />
      </>
    ),
  },
];

export interface RailProps {
  /** Called when an inert (not-yet-wired) rail item is clicked. */
  onInert: (label: string) => void;
}

export function Rail({ onInert }: RailProps) {
  return (
    <nav className="rail" aria-label="Sections">
      {ITEMS.map((item) => (
        <button
          key={item.id}
          type="button"
          className="rail-btn"
          data-tooltip={item.label}
          aria-current={item.active}
          aria-label={item.label}
          title={item.label}
          onClick={() => {
            if (!item.active) onInert(item.label);
          }}
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
