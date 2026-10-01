import { describe, expect, it } from "vitest";
import { act, render, screen } from "@testing-library/react";
import { TrackSlide } from "./TrackSlide";

describe("TrackSlide", () => {
  it("does not animate the first track it is given", () => {
    const { container } = render(<TrackSlide direction={1} slideKey="a"><p>Alpha</p></TrackSlide>);
    expect(container.querySelector(".track-slide")).not.toHaveAttribute("data-slide");
    expect(container.querySelectorAll(".track-slide__layer--leaving")).toHaveLength(0);
  });

  it("keeps the previous track on screen while the next one arrives", () => {
    const view = render(<TrackSlide direction={1} slideKey="a"><p>Alpha</p></TrackSlide>);
    view.rerender(<TrackSlide direction={1} slideKey="b"><p>Bravo</p></TrackSlide>);

    const leaving = view.container.querySelector(".track-slide__layer--leaving");
    expect(leaving).toHaveTextContent("Alpha");
    expect(leaving).toHaveAttribute("aria-hidden", "true");
    expect(view.container.querySelector(".track-slide__layer--current")).toHaveTextContent("Bravo");
    expect(view.container.querySelector(".track-slide")).toHaveAttribute("data-slide", "1");
    // Only the incoming track is reachable; the outgoing copy is decoration.
    expect(screen.getAllByText(/Alpha|Bravo/)).toHaveLength(2);
  });

  it("carries the direction of the skip", () => {
    const view = render(<TrackSlide direction={1} slideKey="a"><p>Alpha</p></TrackSlide>);
    view.rerender(<TrackSlide direction={-1} slideKey="a"><p>Alpha</p></TrackSlide>);
    expect(view.container.querySelector(".track-slide")).not.toHaveAttribute("data-slide");
    view.rerender(<TrackSlide direction={-1} slideKey="z"><p>Zulu</p></TrackSlide>);
    expect(view.container.querySelector(".track-slide")).toHaveAttribute("data-slide", "-1");
  });

  it("retires the outgoing layer once the slide is over", async () => {
    const view = render(<TrackSlide direction={1} slideKey="a"><p>Alpha</p></TrackSlide>);
    view.rerender(<TrackSlide direction={1} slideKey="b"><p>Bravo</p></TrackSlide>);
    expect(view.container.querySelector(".track-slide__layer--leaving")).toBeInTheDocument();
    await act(() => new Promise((resolve) => setTimeout(resolve, 700)));
    expect(view.container.querySelector(".track-slide__layer--leaving")).not.toBeInTheDocument();
    expect(view.container.querySelector(".track-slide__layer--current")).toHaveTextContent("Bravo");
  });

  it("replaces the outgoing layer when a second skip lands mid-slide", () => {
    const view = render(<TrackSlide direction={1} slideKey="a"><p>Alpha</p></TrackSlide>);
    view.rerender(<TrackSlide direction={1} slideKey="b"><p>Bravo</p></TrackSlide>);
    view.rerender(<TrackSlide direction={1} slideKey="c"><p>Charlie</p></TrackSlide>);
    expect(view.container.querySelectorAll(".track-slide__layer--leaving")).toHaveLength(1);
    expect(view.container.querySelector(".track-slide__layer--leaving")).toHaveTextContent("Bravo");
    expect(view.container.querySelector(".track-slide__layer--current")).toHaveTextContent("Charlie");
  });
});
