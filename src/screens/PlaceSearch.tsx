import { useEffect, useState } from "react";

import { Button, Card, Chip, Notice } from "../components/ui";
import { PageTitle, Screen } from "../components/Screen";
import { IconInfo, IconLocate, IconSearch, IconWork } from "../components/icons";
import { getAmapKeyStatus, searchPlace, setAmapKey, type Place } from "../lib/api";
import { useAppStore } from "../lib/store";
import { useLocation } from "../lib/location";
import "./PlaceSearch.css";

/**
 * 地点搜索 —— 用高德补上公交接口做不到的事。
 *
 * # 为什么需要它
 *
 * 公交接口的搜索**只匹配站名子串**：
 * `"常青花园"` ✅ / `"楚天雅苑"` ❌ / `"洪山科创大厦"` ❌。
 * 所以"设置家/公司"靠公交接口根本做不了。
 *
 * 高德 POI 搜索能精确定位这些地点，而且返回的也是 **GCJ-02**，
 * 与公交接口天然对齐——拿到坐标后直接调 `nearBy` 就能找到最近站点。
 */
export function PlaceSearch({
  kind,
  title,
  onDone,
}: {
  kind: "home" | "work";
  title: string;
  onDone?: () => void;
}) {
  const { setPlace } = useAppStore();
  const { fix, nearby, refresh, loading: locating } = useLocation();

  const [keyStatus, setKeyStatus] = useState<{ configured: boolean; masked: string | null } | null>(
    null,
  );
  const [keyInput, setKeyInput] = useState("");
  const [savingKey, setSavingKey] = useState(false);
  const [keyError, setKeyError] = useState<string | null>(null);

  const [keyword, setKeyword] = useState("");
  const [results, setResults] = useState<Place[]>([]);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [searched, setSearched] = useState(false);

  useEffect(() => {
    void getAmapKeyStatus().then(setKeyStatus).catch(() => setKeyStatus(null));
  }, []);

  const saveKey = async () => {
    setSavingKey(true);
    setKeyError(null);
    try {
      const st = await setAmapKey(keyInput);
      setKeyStatus(st);
      setKeyInput("");
    } catch (e) {
      setKeyError(e instanceof Error ? e.message : String(e));
    } finally {
      setSavingKey(false);
    }
  };

  const run = async () => {
    const q = keyword.trim();
    if (!q) return;
    setSearching(true);
    setSearchError(null);
    try {
      // 限定武汉可显著提升准确度（高德城市码 420100）
      const r = await searchPlace(q, "420100");
      setResults(r);
      setSearched(true);
    } catch (e) {
      setSearchError(e instanceof Error ? e.message : String(e));
      setResults([]);
      setSearched(true);
    } finally {
      setSearching(false);
    }
  };

  /** 用当前位置直接保存（不需要高德 Key 的兜底路径） */
  const useCurrent = () => {
    if (!fix) return;
    setPlace({
      kind,
      label: title,
      point: { lng: fix.lng, lat: fix.lat },
      source: "gps",
      nearestStops: nearby.slice(0, 3),
    });
    onDone?.();
  };

  /** 用搜索结果保存，并把附近站点一并记下 */
  const pick = async (p: Place) => {
    setPlace({
      kind,
      label: p.name || title,
      point: p.point,
      source: "amap",
      address: p.address,
    });
    onDone?.();
  };

  return (
    <Screen header={<PageTitle title={`设置${title}`} subtitle="按地址搜索，或用当前位置" />}>
      {/* ---------- 高德 Key ---------- */}
      {keyStatus && !keyStatus.configured ? (
        <Card>
          <div className="card__head">
            <span className="card__title">先配置高德 Key</span>
          </div>
          <p className="t-label t-2" style={{ margin: "0 0 var(--space-3)" }}>
            公交接口只认站名，不认小区名和写字楼名（搜「楚天雅苑」是空的）。
            配置高德 Key 后就能按地址搜索。
          </p>
          <input
            className="ps__input"
            type="text"
            value={keyInput}
            onChange={(e) => setKeyInput(e.target.value)}
            placeholder="粘贴高德 Web 服务 Key"
            autoComplete="off"
            spellCheck={false}
            aria-label="高德 Key"
          />
          <div style={{ marginTop: "var(--space-2)" }}>
            <Button block onClick={() => void saveKey()} disabled={savingKey || !keyInput.trim()}>
              {savingKey ? "保存中…" : "保存"}
            </Button>
          </div>
          {keyError ? (
            <div style={{ marginTop: "var(--space-2)" }}>
              <Notice tone="danger">{keyError}</Notice>
            </div>
          ) : null}
          <div style={{ marginTop: "var(--space-3)" }}>
            <Notice tone="info">
              <IconInfo size={14} />
              <span>
                Key 只保存在这台设备上，不会上传、也不会写进代码。
                到 <b>lbs.amap.com</b> 免费申请，类型选「Web 服务」。
              </span>
            </Notice>
          </div>
        </Card>
      ) : keyStatus?.configured ? (
        <div className="ps__keyrow">
          <Chip tone="live">高德已配置</Chip>
          <span className="t-cap t-3 tnum">{keyStatus.masked}</span>
        </div>
      ) : null}

      {/* ---------- 用当前位置（无需 Key） ---------- */}
      <div style={{ marginTop: "var(--space-3)" }}>
        <Card>
          <div className="row gap-3">
            <IconLocate size={20} className="t-2" />
            <div className="grow">
              <div className="t-head">用当前位置</div>
              <div className="t-cap t-3">
                {fix
                  ? `${fix.lng.toFixed(5)}, ${fix.lat.toFixed(5)}${fix.simulated ? " · 模拟位置" : ""}`
                  : "尚未定位"}
              </div>
            </div>
            {fix ? (
              <Button variant="ghost" size="sm" onClick={useCurrent}>
                就用这里
              </Button>
            ) : (
              <Button variant="ghost" size="sm" onClick={() => void refresh()} disabled={locating}>
                {locating ? "定位中" : "定位"}
              </Button>
            )}
          </div>
          {nearby.length > 0 ? (
            <div className="t-cap t-3" style={{ marginTop: "var(--space-2)" }}>
              附近站点：{nearby.slice(0, 3).map((s) => s.stopName).join(" · ")}
            </div>
          ) : null}
        </Card>
      </div>

      {/* ---------- 按地址搜索 ---------- */}
      {keyStatus?.configured ? (
        <>
          <div className="ps__search">
            <IconSearch size={18} className="ps__search-icon" />
            <input
              className="ps__input ps__input--bare"
              value={keyword}
              onChange={(e) => setKeyword(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void run();
              }}
              placeholder="输入小区名或写字楼名，如 楚天雅苑"
              enterKeyHint="search"
              autoComplete="off"
              autoCorrect="off"
              spellCheck={false}
              aria-label="搜索地点"
            />
          </div>
          <div style={{ marginTop: "var(--space-2)" }}>
            <Button block variant="ghost" onClick={() => void run()} disabled={searching || !keyword.trim()}>
              {searching ? "搜索中…" : "搜索"}
            </Button>
          </div>

          {searchError ? (
            <div style={{ marginTop: "var(--space-3)" }}>
              <Notice tone="danger">{searchError}</Notice>
            </div>
          ) : null}

          {results.length > 0 ? (
            <div className="stack" style={{ marginTop: "var(--space-3)" }}>
              {results.map((p, i) => (
                <button key={i} type="button" className="ps__result" onClick={() => void pick(p)}>
                  <span className="ps__result-icon">
                    <IconWork size={16} />
                  </span>
                  <span className="list__grow">
                    <span className="t-head truncate">{p.name}</span>
                    <span className="t-cap t-3 truncate" style={{ display: "block" }}>
                      {p.district}
                      {p.address ? ` · ${p.address}` : ""}
                    </span>
                    {p.kind ? (
                      <span className="t-cap t-3 truncate" style={{ display: "block" }}>
                        {p.kind}
                      </span>
                    ) : null}
                  </span>
                </button>
              ))}
            </div>
          ) : null}

          {searched && !searching && results.length === 0 && !searchError ? (
            <div className="t-label t-2" style={{ marginTop: "var(--space-3)", textAlign: "center" }}>
              没有找到匹配的地点，换个更完整的名字试试
            </div>
          ) : null}
        </>
      ) : null}
    </Screen>
  );
}
