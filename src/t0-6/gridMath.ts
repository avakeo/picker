export interface VisibleRange {
  columns: number;
  rowCount: number;
  start: number;
  end: number;
}

export function visibleRange(
  itemCount: number,
  viewportWidth: number,
  viewportHeight: number,
  scrollTop: number,
  tileWidth: number,
  tileHeight: number,
  overscanRows: number
): VisibleRange {
  const columns = Math.max(1, Math.floor(viewportWidth / tileWidth));
  const rowCount = Math.ceil(itemCount / columns);
  const firstRow = Math.max(0, Math.floor(scrollTop / tileHeight) - overscanRows);
  const lastRow = Math.min(
    rowCount,
    Math.ceil((scrollTop + viewportHeight) / tileHeight) + overscanRows
  );
  return {
    columns,
    rowCount,
    start: firstRow * columns,
    end: Math.min(itemCount, lastRow * columns)
  };
}
