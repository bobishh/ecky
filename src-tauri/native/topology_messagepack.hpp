#pragma once

#include <cmath>
#include <cstdint>
#include <cstring>
#include <limits>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace ecky {

// Topology uses named MessagePack maps so old and new projections can share
// the same field names. A container's count is filled in after enumeration.
class TopologyMessagePack {
public:
    void object() { begin(0xdf, true); }
    void array() { begin(0xdd, false); }

    void end() {
        const Container item = containers_.back();
        containers_.pop_back();
        for (int index = 0; index < 4; ++index) {
            data_[item.count_offset + index] =
                static_cast<char>(item.count >> (24 - index * 8));
        }
    }

    void key(std::string_view name) {
        ++containers_.back().count;
        write_text(name);
    }

    void text(std::string_view value) {
        add_item();
        write_text(value);
    }

    void integer(std::uint32_t value) {
        add_item();
        if (value < 0x80) {
            byte(value);
        } else if (value <= 0xffff) {
            byte(0xcd);
            word(value, 2);
        } else {
            byte(0xce);
            word(value, 4);
        }
    }

    void boolean(bool value) {
        add_item();
        byte(value ? 0xc3 : 0xc2);
    }

    void number(double value) {
        add_item();
        constexpr std::uint64_t zero = 0;
        std::uint64_t bits = zero;
        static_assert(sizeof(bits) == sizeof(value));
        if (std::isfinite(value)) std::memcpy(&bits, &value, sizeof(value));
        byte(0xcb);
        word(bits, 8);
    }

    // When streaming a known-length array, the caller writes exactly count
    // complete MessagePack values immediately after this prefix.
    void array_prefix(std::size_t count) {
        if (count > std::numeric_limits<std::uint32_t>::max()) {
            throw std::length_error("topology array exceeds MessagePack length");
        }
        add_item();
        byte(0xdd);
        word(count, 4);
    }

    const std::string& data() const { return data_; }
    std::string take_data() { return std::move(data_); }

private:
    struct Container {
        std::size_t count_offset;
        std::uint32_t count;
        bool is_object;
    };

    void begin(unsigned int tag, bool is_object) {
        add_item();
        byte(tag);
        containers_.push_back({data_.size(), 0, is_object});
        word(0, 4);
    }

    void add_item() {
        if (!containers_.empty() && !containers_.back().is_object) {
            ++containers_.back().count;
        }
    }

    void write_text(std::string_view value) {
        const std::size_t size = value.size();
        if (size < 32) {
            byte(0xa0 | size);
        } else if (size <= 0xff) {
            byte(0xd9);
            word(size, 1);
        } else if (size <= 0xffff) {
            byte(0xda);
            word(size, 2);
        } else if (size <= std::numeric_limits<std::uint32_t>::max()) {
            byte(0xdb);
            word(size, 4);
        } else {
            throw std::length_error("topology string exceeds MessagePack length");
        }
        data_.append(value);
    }

    void byte(std::uint64_t value) { data_.push_back(static_cast<char>(value & 0xff)); }

    void word(std::uint64_t value, int size) {
        for (int index = size - 1; index >= 0; --index) {
            byte(value >> (8 * index));
        }
    }

    std::string data_;
    std::vector<Container> containers_;
};

}  // namespace ecky
