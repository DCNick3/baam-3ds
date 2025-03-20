#include <cstdint>

// for osGetTime
#include <3ds.h>

#include <pqrs/binarization.h>
#include <pqrs/linear_external_contours.h>
#include <pqrs/interpolation.h>
#include <pqrs/contour_edge_intensity.h>
#include <pqrs/contour_to_tetragon.h>
#include <pqrs/finder_pattern_detector.h>
#include <pqrs/homography_dlt.h>
#include <pqrs/qr_scanner.h>
#include <pqrs/qr_bitstream_reader.h>
#include <pqrs/qr_grid_reader.h>

typedef struct {
    u64 binarize;
    u64 linear_external_contours;
    u64 contour_processing;
    u64 qr_detection;
    u64 qr_decoding;
    
    u64 total() {
        return binarize + linear_external_contours + contour_processing + qr_detection + qr_decoding;
    }
} processing_timings;

static std::vector<std::string> process_frame(pqrs::gray_u8 const& gray, processing_timings& timings) {
	uint64_t start_time;

    // TODO: probably reallocating these for every processed frame is not the best
	start_time = osGetTime();
    auto bin = pqrs::binarize(gray, 0.005);
    timings.binarize = osGetTime() - start_time;
	
	start_time = osGetTime();
    auto contours = pqrs::linear_external_contours_in_place(bin);
    timings.linear_external_contours = osGetTime() - start_time;

    std::vector<pqrs::finder_pattern> finder_patterns;

	start_time = osGetTime();
    for (auto const& contour : contours) {
        auto intensivity = pqrs::contour_edge_intensity(gray, contour);
        auto diff = std::abs(intensivity.first - intensivity.second);
        if (diff > 3) {
            //std::cerr << intensivity.first << " / " << intensivity.second << ": ";

            auto tetragon = pqrs::contour_to_tetragon(contour);
            std::optional<pqrs::finder_pattern> position_pattern;
            if (tetragon) {
                position_pattern = pqrs::check_finder_pattern(*tetragon, gray);
            }

            //std::cerr << (tetragon ? "T" : "-")
            //          << (position_pattern ? "P" : "-") << " ";

            for (int i = 0; i < contour.size(); i++) {

                //std::cerr << "{ " << contour[i].x() << ", " << contour[i].y() << " }";

                //img(contour[i].y(), contour[i].x(), 0) = 0;
                //img(contour[i].y(), contour[i].x(), 1) = 0;
                //img(contour[i].y(), contour[i].x(), 2) = 255;

                //if (i != contour.size() - 1) {
                //    std::cerr << ", ";
                //}
            }
            //std::cerr << std::endl;

            if (tetragon) {
                for (auto p : *tetragon) {
                    //img(p.y(), p.x(), 0) = 0;
                    //img(p.y(), p.x(), 1) = 255;
                    //img(p.y(), p.x(), 2) = 0;
                }
            }

            if (position_pattern) {
                //img(position_pattern->center.y(), position_pattern->center.x(), 0) = 255;
                //img(position_pattern->center.y(), position_pattern->center.x(), 1) = 0;
                //img(position_pattern->center.y(), position_pattern->center.x(), 2) = 0;

                finder_patterns.emplace_back(*position_pattern);
            }
        }
    }
    timings.contour_processing = osGetTime() - start_time;

	start_time = osGetTime();
    auto qr_codes = pqrs::detect_qr_codes(gray, finder_patterns);
    timings.qr_detection = osGetTime() - start_time;

	start_time = osGetTime();

	std::vector<std::string> strings;

    for (auto const& qr : qr_codes) {
        auto sz = qr.size();

        // auto grid = pqrs::qr_grid_global(gray, qr._homography, sz);
        // 
        // for (int i = 0; i < sz; i++) {
        //     for (int j = 0; j < sz; j++) {
        //         auto pt = qr._homography.map({(float)i + .5f, (float)j + .5f});
        //         auto val = grid.sample(i, j);
        // 
        //         //img(pt.y(), pt.x(), 0) = val ? 255 : 0;
        //         //img(pt.y(), pt.x(), 1) = val ? 0 : 255;
        //         //img(pt.y(), pt.x(), 2) = 255;
        //     }
        // }

        auto scanned = pqrs::decode_qr_code(gray, qr);

        if (scanned) {
			strings.push_back(std::move(scanned->_decoded_content));
        }
    }
    timings.qr_decoding = osGetTime() - start_time;

    return strings;
}

struct context_t {
    std::array<std::size_t, 2> shape;
    pqrs::gray_u8 gray;
    std::vector<std::string> strings;
    std::vector<const char*> string_ptrs;
};

extern "C" void* alloc_qr_processing_context(std::uint32_t width, std::uint32_t height) {
    std::array<std::size_t, 2> shape = { static_cast<std::size_t>(height), static_cast<std::size_t>(width) };
    pqrs::gray_u8 gray(shape);

    return (void*) new context_t{std::move(shape), std::move(gray)};
}

extern "C" void free_qr_processing_context(void* raw_context) {
    delete static_cast<context_t*>(raw_context);
}

extern "C" void qr_processing_set_frame(void* raw_context, char const* yuv_ptr) {
    context_t* context = static_cast<context_t*>(raw_context);

    for (int j = 0; j < context->shape[0]; ++j) {
        for (int i = 0; i < context->shape[1]; ++i) {
            // for each 2 pixels this stores
            // y u y v
            // ^   ^
            // these are what we need for gray image
            uint8_t luma = yuv_ptr[(j * context->shape[1] + i) * 2];
            context->gray(j, i) = luma;
        }
    }
}

extern "C" void qr_processing_process(void* raw_context, processing_timings* timings) {
    context_t* context = static_cast<context_t*>(raw_context);

    context->strings = process_frame(context->gray, *timings);
    context->string_ptrs.clear();
    for (const auto& s : context->strings) {
        context->string_ptrs.push_back(s.c_str());
    }
}

extern "C" void qr_processing_get_strings(void* raw_context, char const*** out_ptr, std::uint32_t* out_count) {
    context_t* context = static_cast<context_t*>(raw_context);

    *out_ptr = context->string_ptrs.data();
    *out_count = context->string_ptrs.size();
}
