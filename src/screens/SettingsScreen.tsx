import { useEffect, useState } from "react";

import { Button, Card, Chip, Notice, Segmented } from "../components/ui";
import { PageTitle, Screen, SectionTitle } from "../components/Screen";
import {
  IconBus,
  IconInfo,
  IconLocate,
  IconMoon,
  IconRefresh,
  IconStar,
} from "../components/icons";
import { getApiState } from "../lib/api";
import { useAppStore } from "../lib/store";
import { useLocation } from "../lib/location";
import type { ApiState } from "../lib/types";
import type { ThemeMode } from "../App";
import "./SettingsScreen.css";

export function SettingsScreen({
  theme,
  onThemeChange,
}: {
  theme: ThemeMode;
  onThemeChange: (t: ThemeMode) => void;
}) {
  const { favorites, places, clearPlace } = useAppStore();
  const { fix, nearby, loading, refresh } = useLocation();
  const [api, setApi] = useState<ApiState | null>(null);
  const [apiErr, setApiErr] = useState<string | null>(null);

  const loadApi = async () => {
    try {
      setApi(await getApiState());
      setApiErr(null);
    } catch (e) {
      setApiErr(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    void loadApi();
  }, []);

  return (
    <Screen header={<PageTitle title="我的" subtitle="偏好设置与数据来源" />}>
      {/* ---------- 外观 ---------- */}
      <section>
        <SectionTitle>外观</SectionTitle>
        <Card>
          <Segmented
            value={theme}
            ariaLabel="主题"
            onChange={onThemeChange}
            options={[
              { value: "system", label: "跟随系统" },
              { value: "light", label: "浅色" },
              { value: "dark", label: "深色" },
            ]}
          />
          <div className="t-cap t-3" style={{ marginTop: "var(--space-2)" }}>
            深浅两套主题的正文对比度均已独立验证达标。
          </div>
        </Card>
      </section>

      {/* ---------- 定位诊断 ---------- */}
      <section>
        <SectionTitle
          action={
            <button
              type="button"
              className="t-cap"
              style={{ color: "var(--brand)", minHeight: 32 }}
              onClick={() => void refresh()}
              disabled={loading}
            >
              <IconRefresh size={13} /> {loading ? "定位中" : "重新定位"}
            </button>
          }
        >
          定位
        </SectionTitle>
        <Card>
          {fix ? (
            <div className="kv">
              <Row label="当前坐标">
                <span className="tnum">
                  {fix.lng.toFixed(6)}, {fix.lat.toFixed(6)}
                </span>
              </Row>
              <Row label="坐标系">
                <Chip tone="brand">GCJ-02</Chip>
              </Row>
              <Row label="来源">
                {fix.simulated ? (
                  <Chip tone="warn">模拟位置</Chip>
                ) : (
                  <Chip tone="live">
                    <IconLocate size={12} /> 系统定位
                  </Chip>
                )}
              </Row>
              <Row label="精度">
                <span className="tnum">±{Math.round(fix.accuracy)} 米</span>
              </Row>
              <Row label="周边站点">
                <span className="tnum">{nearby.length} 个</span>
              </Row>
            </div>
          ) : (
            <div className="t-label t-2">尚未定位</div>
          )}

          <div style={{ marginTop: "var(--space-3)" }}>
            <Notice tone="info">
              <IconInfo size={14} />
              <span>
                系统定位返回 WGS-84，公交换乘接口使用 GCJ-02。
                本 App 在 Rust 侧完成转换——武汉地区两者相差约 <b>587 米</b>，
                而接口的周边搜索半径只有约 400 米，不转换会查到错误的站点。
              </span>
            </Notice>
          </div>
        </Card>
      </section>

      {/* ---------- 常用地点 ---------- */}
      <section>
        <SectionTitle>常用地点</SectionTitle>
        <Card>
          {(["home", "work"] as const).map((k) => {
            const p = places[k];
            return (
              <div key={k} className="settings__row">
                <span className="t-label">{k === "home" ? "家" : "公司"}</span>
                <span className="grow t-cap t-3 truncate">
                  {p ? `${p.point.lng.toFixed(4)}, ${p.point.lat.toFixed(4)}` : "未设置"}
                </span>
                {p ? (
                  <Button variant="quiet" size="sm" onClick={() => clearPlace(k)}>
                    清除
                  </Button>
                ) : null}
              </div>
            );
          })}
          <div className="t-cap t-3" style={{ marginTop: "var(--space-2)" }}>
            到「规划」页可以用当前位置一键保存。
          </div>
        </Card>
      </section>

      {/* ---------- 收藏 ---------- */}
      <section>
        <SectionTitle>{`收藏线路 · ${favorites.length}`}</SectionTitle>
        <Card>
          {favorites.length === 0 ? (
            <div className="t-label t-2">还没有收藏线路</div>
          ) : (
            <div className="kv">
              {favorites.map((f) => (
                <Row key={f.lineId} label={f.lineName}>
                  <span className="t-cap t-2 truncate">
                    {f.startStop} → {f.endStop}
                  </span>
                </Row>
              ))}
            </div>
          )}
        </Card>
      </section>

      {/* ---------- 数据通道诊断 ---------- */}
      <section>
        <SectionTitle
          action={
            <button
              type="button"
              className="t-cap"
              style={{ color: "var(--brand)", minHeight: 32 }}
              onClick={() => void loadApi()}
            >
              <IconRefresh size={13} /> 检查
            </button>
          }
        >
          数据通道
        </SectionTitle>
        <Card>
          {apiErr ? (
            <Notice tone="danger">{apiErr}</Notice>
          ) : (
            <div className="kv">
              <Row label="密钥状态">
                {api?.keyReady ? (
                  <Chip tone="live">已就绪</Chip>
                ) : (
                  <Chip tone="neutral">未获取</Chip>
                )}
              </Row>
              <Row label="密钥版本">
                <span className="tnum">{api?.secretVersion ?? "—"}</span>
              </Row>
              <Row label="服务地址">
                <span className="t-cap t-3">customized.wuhanbus.com</span>
              </Row>
            </div>
          )}
          <div className="t-cap t-3" style={{ marginTop: "var(--space-2)" }}>
            密钥每日轮换，过期后自动重新获取。
          </div>
        </Card>
      </section>

      {/* ---------- 关于 ---------- */}
      <section>
        <SectionTitle>关于</SectionTitle>
        <Card>
          <div className="kv">
            <Row label="版本">
              <span className="tnum">0.1.0</span>
            </Row>
            <Row label="数据来源">
              <span className="t-cap t-2">武汉公交 gps-resapi（第三方）</span>
            </Row>
            <Row label="坐标系">
              <span className="t-cap t-2">全程 GCJ-02</span>
            </Row>
          </div>
          <div style={{ marginTop: "var(--space-3)" }}>
            <Notice tone="warn">
              本站数据来自第三方非官方接口，仅供个人通勤参考。
              车辆位置可能有延迟，请以实际到站情况为准。
            </Notice>
          </div>
        </Card>
      </section>

      <div className="settings__foot">
        <IconBus size={14} />
        <span>whbus · 开源项目</span>
        <IconStar size={14} />
        <IconMoon size={14} />
      </div>
    </Screen>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="settings__row">
      <span className="t-label t-2">{label}</span>
      <span className="grow" />
      <span className="t-label">{children}</span>
    </div>
  );
}
