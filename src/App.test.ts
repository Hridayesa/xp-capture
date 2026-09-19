import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SelfCheckClientError, runSelfCheck } from "./api/self-check";
import App from "./App.vue";

vi.mock("./api/self-check", async (importOriginal) => {
  const original = await importOriginal<typeof import("./api/self-check")>();
  return { ...original, runSelfCheck: vi.fn() };
});

const mockedRunSelfCheck = vi.mocked(runSelfCheck);
const successfulReport = {
  schema_version: 1,
  checks: [
    { check: "opencv_load" as const, status: "passed" as const },
    { check: "image_codec" as const, status: "passed" as const },
    { check: "writer_open" as const, status: "passed" as const },
    { check: "writer_backend" as const, status: "passed" as const },
    { check: "writer_roundtrip" as const, status: "passed" as const },
  ],
  opencv_version: "4.12.0",
};

beforeEach(() => {
  mockedRunSelfCheck.mockReset();
});

describe("App", () => {
  it("shows an idle camera-independent diagnostic screen", () => {
    const wrapper = mount(App);

    expect(wrapper.get("h1").text()).toBe("XP Capture");
    expect(wrapper.text()).toContain("без обращения к камере");
    expect(wrapper.get("button").text()).toBe("Запустить");
  });

  it("shows a responsive loading state while native work is pending", async () => {
    mockedRunSelfCheck.mockReturnValue(new Promise(() => undefined));
    const wrapper = mount(App);

    await wrapper.get("button").trigger("click");

    expect(wrapper.get('[role="status"]').text()).toContain("Интерфейс остаётся доступным");
    expect(wrapper.get("button").attributes("disabled")).toBeDefined();
  });

  it("renders every successful check separately", async () => {
    mockedRunSelfCheck.mockResolvedValue(successfulReport);
    const wrapper = mount(App);

    await wrapper.get("button").trigger("click");
    await flushPromises();

    expect(wrapper.findAll(".check-list li")).toHaveLength(5);
    expect(wrapper.text()).toContain("OpenCV 4.12.0");
    expect(wrapper.findAll(".status-passed")).toHaveLength(5);
  });

  it("shows a safe native error and supports retry", async () => {
    mockedRunSelfCheck
      .mockRejectedValueOnce(
        new SelfCheckClientError("internal_error", "Внутренняя ошибка самопроверки."),
      )
      .mockResolvedValueOnce(successfulReport);
    const wrapper = mount(App);

    await wrapper.get("button").trigger("click");
    await flushPromises();
    expect(wrapper.get('[role="alert"]').text()).toContain("Внутренняя ошибка");
    expect(wrapper.get("button").text()).toBe("Повторить");

    await wrapper.get("button").trigger("click");
    await flushPromises();
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
    expect(wrapper.findAll(".check-list li")).toHaveLength(5);
    expect(mockedRunSelfCheck).toHaveBeenCalledTimes(2);
  });
});
