<script lang="ts">
  /** 腾讯 GL 矢量底图：底图自带道路、地铁线路与站点标注；AutoBee 叠加当前路线。 */
  import { planner } from "$lib/state/planner.svelte";
  import type { RoutePlan } from "$lib/ipc/types";

  type TencentMap = { setCenter(center: unknown): void; setZoom(zoom: number): void; getZoom(): number; fitBounds(bounds: unknown, options?: { padding: number }): void };
  type Polyline = { setStyles(styles: Record<string, unknown>): void; setGeometries(geometries: unknown[]): void };
  type Marker = { setStyles(styles: Record<string, unknown>): void; setGeometries(geometries: unknown[]): void };
  type TencentApi = {
    LatLng: new (lat: number, lng: number) => unknown;
    LatLngBounds: new (southWest: unknown, northEast: unknown) => unknown;
    Map: new (element: HTMLElement, options: Record<string, unknown>) => TencentMap;
    MultiPolyline: new (options: Record<string, unknown>) => Polyline;
    MultiMarker: new (options: Record<string, unknown>) => Marker;
    PolylineStyle: new (options: Record<string, unknown>) => unknown;
    MarkerStyle: new (options: Record<string, unknown>) => unknown;
  };

  let { class: className = "" }: { class?: string } = $props();
  let container = $state<HTMLDivElement | null>(null);
  let map: TencentMap | null = null;
  let polylineLayer: Polyline | null = null;
  let markerLayer: Marker | null = null;
  let loadError = $state("");
  let mapReady = $state(false);

  function sdk(): TencentApi | undefined {
    return (window as unknown as { TMap?: TencentApi }).TMap;
  }

  $effect(() => {
    if (container) void initMap(container);
  });

  $effect(() => {
    void planner.selectedRouteId;
    void planner.circleResults;
    if (map && mapReady) {
      renderPlan(planner.plan);
      renderMarkers(planner.plan);
    }
  });

  async function initMap(element: HTMLElement): Promise<void> {
    try {
      const TMap = await waitForSdk();
      const center = new TMap.LatLng(39.90868, 116.3953);
      map = new TMap.Map(element, { center, zoom: 12, baseMap: { type: "vector" } });
      polylineLayer = new TMap.MultiPolyline({ map, styles: {}, geometries: [] });
      markerLayer = new TMap.MultiMarker({ map, styles: {}, geometries: [] });
      mapReady = true;
      renderPlan(planner.plan);
    } catch (error) {
      loadError = error instanceof Error ? error.message : "腾讯地图加载失败";
    }
  }

  function waitForSdk(): Promise<TencentApi> {
    const existing = sdk();
    if (existing) return Promise.resolve(existing);
    return new Promise((resolve, reject) => {
      const started = Date.now();
      const timer = window.setInterval(() => {
        const current = sdk();
        if (current) {
          window.clearInterval(timer);
          resolve(current);
        } else if (Date.now() - started > 15_000) {
          window.clearInterval(timer);
          reject(new Error("腾讯地图 SDK 加载超时"));
        }
      }, 50);
    });
  }

  function renderPlan(plan: RoutePlan | null): void {
    const TMap = sdk();
    if (!map || !polylineLayer || !TMap) return;
    if (!plan || plan.routes.length === 0) {
      polylineLayer.setGeometries([]);
      return;
    }
    const route = plan.routes.find((item) => item.id === planner.selectedRouteId) ?? plan.routes[0];
    const styles: Record<string, unknown> = {};
    const geometries: unknown[] = [];
    let lineIndex = 0;
    for (const segment of route.segments) {
      if (segment.points.length < 2) continue;
      const id = `segment-${lineIndex++}`;
      styles[id] = new TMap.PolylineStyle({ color: segment.line?.color ?? "#9aa0a6", width: segment.kind === "walk" ? 4 : 7, borderWidth: segment.kind === "walk" ? 1 : 2, borderColor: "#ffffff", lineCap: "round", lineJoin: "round" });
      geometries.push({ id, styleId: id, paths: segment.points.map((p) => new TMap.LatLng(p.lat, p.lng)) });
    }
    polylineLayer.setStyles(styles);
    polylineLayer.setGeometries(geometries);
    const routeBounds = route.bounds;
    const boundsAreValid = routeBounds.every((point) =>
      Number.isFinite(point.lat) && Number.isFinite(point.lng)
      && Math.abs(point.lat) <= 90 && Math.abs(point.lng) <= 180,
    );
    const southWest = boundsAreValid
      ? new TMap.LatLng(routeBounds[0].lat, routeBounds[0].lng)
      : new TMap.LatLng(plan.origin.location.lat, plan.origin.location.lng);
    const northEast = boundsAreValid
      ? new TMap.LatLng(routeBounds[1].lat, routeBounds[1].lng)
      : new TMap.LatLng(plan.destination.location.lat, plan.destination.location.lng);
    map.fitBounds(new TMap.LatLngBounds(southWest, northEast), { padding: 72 });
    map.setCenter(new TMap.LatLng(
      (plan.origin.location.lat + plan.destination.location.lat) / 2,
      (plan.origin.location.lng + plan.destination.location.lng) / 2,
    ));
    if (map.getZoom() > 12) map.setZoom(12);
  }

  /**
   * 端点与生活圈服务点的打点。跟 `renderPlan` 分开是必须的:生活圈现在有
   * 自己的地点输入,没规划过路线时也要能在图上看到服务点 -- 写在 renderPlan
   * 里会被它的 `!plan` 提前返回吞掉。
   */
  function renderMarkers(plan: RoutePlan | null): void {
    const TMap = sdk();
    if (!map || !markerLayer || !TMap) return;
    markerLayer.setStyles({
      origin: new TMap.MarkerStyle({ width: 28, height: 36, anchor: { x: 14, y: 36 }, src: pin("#16a34a") }),
      destination: new TMap.MarkerStyle({ width: 28, height: 36, anchor: { x: 14, y: 36 }, src: pin("#dc2626") }),
      circle: new TMap.MarkerStyle({ width: 28, height: 36, anchor: { x: 14, y: 36 }, src: pin("#7c3aed") }),
      service: new TMap.MarkerStyle({ width: 14, height: 14, anchor: { x: 7, y: 7 }, src: dot("#2563eb") }),
    });
    const geometries: unknown[] = [];
    if (plan) {
      geometries.push({ id: "origin", styleId: "origin", position: new TMap.LatLng(plan.origin.location.lat, plan.origin.location.lng) });
      geometries.push({ id: "destination", styleId: "destination", position: new TMap.LatLng(plan.destination.location.lat, plan.destination.location.lng) });
    }
    const pois = planner.reachablePois;
    const center = planner.circleOrigin;
    if (center && pois.length > 0 && !plan) {
      geometries.push({ id: "circle-center", styleId: "circle", position: new TMap.LatLng(center.location.lat, center.location.lng) });
    }
    for (const poi of pois) {
      geometries.push({ id: `service-${poi.category}-${poi.name}`, styleId: "service", position: new TMap.LatLng(poi.location.lat, poi.location.lng) });
    }
    markerLayer.setGeometries(geometries);
    // 没有路线时视野不会被 renderPlan 摆好,自己对到生活圈中心上。
    if (!plan && center && pois.length > 0) {
      map.setCenter(new TMap.LatLng(center.location.lat, center.location.lng));
      if (map.getZoom() < 14) map.setZoom(14);
    }
  }


  function pin(color: string): string {
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="28" height="36"><path d="M14 0C6.3 0 0 6.3 0 14c0 10 14 22 14 22s14-12 14-22C28 6.3 21.7 0 14 0z" fill="${color}"/><circle cx="14" cy="13.5" r="5.5" fill="#fff"/></svg>`;
    return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  }

  function dot(color: string): string {
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14"><circle cx="7" cy="7" r="5" fill="#fff" stroke="${color}" stroke-width="3"/></svg>`;
    return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  }

  function zoomBy(delta: number): void {
    if (map) map.setZoom(map.getZoom() + delta);
  }
</script>

<div bind:this={container} class={`relative overflow-hidden rounded-lg border ${className}`}>
  {#if loadError}<div class="absolute inset-0 z-10 grid place-items-center bg-muted p-6 text-center text-sm text-muted-foreground">{loadError}</div>{/if}
  <div class="absolute right-3 bottom-3 z-10 flex flex-col overflow-hidden rounded-md border bg-card shadow-sm">
    <button type="button" aria-label="放大" class="size-8 text-lg hover:bg-accent" onclick={() => zoomBy(1)}>+</button>
    <button type="button" aria-label="缩小" class="size-8 border-t text-lg hover:bg-accent" onclick={() => zoomBy(-1)}>−</button>
  </div>
</div>
