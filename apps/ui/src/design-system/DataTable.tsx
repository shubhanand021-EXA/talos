import type { ReactNode } from "react";

export interface Column<T> {
  header: string;
  render: (row: T) => ReactNode;
}

interface DataTableProps<T> {
  rows: T[];
  columns: Array<Column<T>>;
  keyOf: (row: T, index: number) => string;
  caption: string;
  onRowSelect?: (row: T) => void;
}

export function DataTable<T>({
  rows,
  columns,
  keyOf,
  caption,
  onRowSelect,
}: DataTableProps<T>) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full border-collapse text-left text-sm">
        <caption className="sr-only">{caption}</caption>
        <thead>
          <tr className="border-b border-white/10">
            {columns.map((column) => (
              <th
                key={column.header}
                scope="col"
                className="px-3 py-2 text-xs font-medium uppercase tracking-wider text-slate-500"
              >
                {column.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => (
            <tr
              key={keyOf(row, index)}
              onClick={
                onRowSelect
                  ? () => {
                      onRowSelect(row);
                    }
                  : undefined
              }
              className={
                onRowSelect
                  ? "cursor-pointer border-b border-white/5 transition-colors hover:bg-white/5"
                  : "border-b border-white/5"
              }
            >
              {columns.map((column) => (
                <td key={column.header} className="px-3 py-2.5 text-slate-200">
                  {column.render(row)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
