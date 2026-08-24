package com.sidiora.layerx.sdk;

import java.util.ArrayList;
import java.util.ArrayDeque;
import java.util.HashSet;
import java.util.List;
import java.util.Objects;
import java.util.Queue;
import java.util.Set;
import java.util.concurrent.CompletionException;
import java.util.concurrent.CompletionStage;
import java.util.concurrent.Flow;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicLong;

/** Cursor-checked stream state with atomic page acceptance and virtual-thread-friendly async I/O. */
public final class ResumableStream<T> {
    private static final int MAX_SEEN_EVENT_IDS = 65_536;
    public record Cursor(String value) {
        public Cursor {
            if (value == null || value.isEmpty() || value.length() > 512 || value.indexOf('\0') >= 0) {
                throw PlatformSdkException.invalidArgument();
            }
        }
        @Override public String toString() { return value; }
    }
    public record Event<T>(String eventId, Cursor previousCursor, Cursor cursor, T value) {}
    public record Page<T>(Cursor requestedCursor, List<Event<T>> events, Cursor nextCursor) {
        public Page {
            Objects.requireNonNull(requestedCursor, "requestedCursor");
            events = List.copyOf(events);
            Objects.requireNonNull(nextCursor, "nextCursor");
        }
    }
    @FunctionalInterface public interface PageSource<T> { CompletionStage<Page<T>> fetch(Cursor cursor); }

    private Cursor cursor;
    private final Set<String> seen = new HashSet<>();

    public ResumableStream(Cursor cursor) { this.cursor = Objects.requireNonNull(cursor, "cursor"); }
    public synchronized Cursor cursor() { return cursor; }

    public synchronized List<Event<T>> accept(Page<T> page) {
        Objects.requireNonNull(page, "page");
        if (!page.requestedCursor().equals(cursor)) throw decodeFailure();
        Cursor expected = cursor;
        Set<String> pageIds = new HashSet<>();
        for (Event<T> event : page.events()) {
            if (event == null || event.eventId() == null || event.eventId().isEmpty()
                    || event.previousCursor() == null || event.cursor() == null
                    || !event.previousCursor().equals(expected)
                    || seen.contains(event.eventId()) || !pageIds.add(event.eventId())) throw decodeFailure();
            expected = event.cursor();
        }
        if (!page.nextCursor().equals(expected)) throw decodeFailure();
        if (seen.size() + pageIds.size() > MAX_SEEN_EVENT_IDS) throw decodeFailure();
        seen.addAll(pageIds);
        cursor = page.nextCursor();
        return List.copyOf(new ArrayList<>(page.events()));
    }

    public Flow.Publisher<Event<T>> publisher(PageSource<T> source) {
        Objects.requireNonNull(source, "source");
        AtomicBoolean subscribed = new AtomicBoolean();
        return subscriber -> {
            Objects.requireNonNull(subscriber, "subscriber");
            if (!subscribed.compareAndSet(false, true)) {
                subscriber.onSubscribe(new Flow.Subscription() {
                    @Override public void request(long count) {}
                    @Override public void cancel() {}
                });
                subscriber.onError(new IllegalStateException("a resumable stream publisher permits one subscriber"));
                return;
            }
            var subscription = new StreamSubscription(subscriber, source);
            subscriber.onSubscribe(subscription);
            subscription.start();
        };
    }

    private final class StreamSubscription implements Flow.Subscription, Runnable {
        private final Flow.Subscriber<? super Event<T>> subscriber;
        private final PageSource<T> source;
        private final AtomicLong demand = new AtomicLong();
        private final AtomicBoolean cancelled = new AtomicBoolean();
        private final Queue<Event<T>> pending = new ArrayDeque<>();
        private final Object signal = new Object();

        private StreamSubscription(Flow.Subscriber<? super Event<T>> subscriber, PageSource<T> source) {
            this.subscriber = subscriber;
            this.source = source;
        }

        private void start() { Thread.ofVirtual().name("layerx-stream").start(this); }

        @Override public void request(long count) {
            if (count <= 0) {
                if (cancelled.compareAndSet(false, true)) {
                    subscriber.onError(new IllegalArgumentException("demand must be positive"));
                    wake();
                }
                return;
            }
            demand.getAndUpdate(current -> current > Long.MAX_VALUE - count ? Long.MAX_VALUE : current + count);
            wake();
        }

        @Override public void cancel() {
            cancelled.set(true);
            wake();
        }

        @Override public void run() {
            try {
                while (!cancelled.get()) {
                    awaitDemand();
                    if (cancelled.get()) return;
                    if (pending.isEmpty()) {
                        Page<T> page = Objects.requireNonNull(source.fetch(cursor()), "page stage")
                            .toCompletableFuture().join();
                        pending.addAll(accept(page));
                    }
                    while (demand.get() > 0 && !pending.isEmpty() && !cancelled.get()) {
                        Event<T> event = pending.remove();
                        demand.decrementAndGet();
                        subscriber.onNext(event);
                    }
                }
            } catch (Throwable failure) {
                if (cancelled.compareAndSet(false, true)) subscriber.onError(unwrap(failure));
            }
        }

        private void awaitDemand() throws InterruptedException {
            synchronized (signal) {
                while (demand.get() == 0 && !cancelled.get()) signal.wait();
            }
        }

        private void wake() {
            synchronized (signal) { signal.notifyAll(); }
        }

        private Throwable unwrap(Throwable failure) {
            if (failure instanceof CompletionException completion && completion.getCause() != null) {
                return completion.getCause();
            }
            return failure;
        }
    }

    private static PlatformSdkException decodeFailure() {
        return new PlatformSdkException(PlatformSdkException.Code.DECODE_FAILURE,
            PlatformSdkException.Retry.NEVER, null, null, null);
    }
}
