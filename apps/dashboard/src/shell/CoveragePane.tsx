import { CoverageMatrix } from '../components/CoverageMatrix';

/** Rail "Coverage" view. CoverageMatrix fetches its own data on mount. */
export function CoveragePane() {
  return (
    <div className="view-region">
      <div className="view-stack">
        <CoverageMatrix />
      </div>
    </div>
  );
}
