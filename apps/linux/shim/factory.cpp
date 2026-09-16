// fcitx5 addon 工厂：把 QingjianEngine 注册成 addon。
#include "engine.h"

#include <fcitx/addonfactory.h>
#include <fcitx/addoninstance.h>
#include <fcitx/addonmanager.h>

namespace qingjian {

class QingjianFactory : public fcitx::AddonFactory {
    fcitx::AddonInstance* create(fcitx::AddonManager* manager) override {
        auto* instance = manager->instance();
        return new QingjianEngine(instance);
    }
};

} // namespace qingjian

FCITX_ADDON_FACTORY(qingjian::QingjianFactory)
