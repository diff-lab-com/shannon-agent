import { Outlet } from 'react-router-dom';

export default function Settings() {
  return (
    <div className="flex-1 w-full bg-background overflow-y-auto h-full pb-8">
      <div className="max-w-[800px] mx-auto px-lg py-xl animate-in fade-in duration-700">
        <div className="pt-4">
          <Outlet />
        </div>
      </div>
    </div>
  );
}
