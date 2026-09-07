import { useHome } from './model/store';

/**
 * The Train: strips, standup, track, dock, ambient. Surfaces land as the hi-fi is approved;
 * until then this is the frame, so the store and protocol stay typechecked.
 */
export function App() {
  const connection = useHome((s) => s.connection);
  const directions = useHome((s) => s.directions);
  const count = Object.keys(directions).length;
  return (
    <div className="h-full grid" style={{ gridTemplateRows: 'auto auto 1fr auto', gridTemplateColumns: '1fr 28px' }}>
      <section aria-label="strips" className="row-start-1 col-start-1" />
      <section aria-label="standup" className="row-start-2 col-start-1" />
      <section aria-label="track" className="row-start-3 col-start-1" />
      <section aria-label="dock" className="row-start-4 col-start-1" />
      <aside aria-label="ambient" className="row-span-4 col-start-2 text-muted text-[11px]" title={`${connection}, ${count} directions`} />
    </div>
  );
}
