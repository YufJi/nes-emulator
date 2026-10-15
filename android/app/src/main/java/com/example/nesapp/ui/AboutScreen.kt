package com.example.nesapp.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/**
 * The disclaimer, in the same words the iOS app and `DISCLAIMER.md` use.
 *
 * The statement is not decoration: this app ships no game files and is not
 * affiliated with anyone, and the screen that says so should be as easy to reach
 * as the settings — one press from the list, never buried.
 */
@Composable
fun AboutScreen() {
    Column(Modifier.fillMaxSize().background(Retro.Tube)) {
        TitleBar("关于与免责声明")

        Column(
            Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp, vertical = 18.dp),
        ) {
            Paragraph(
                "本应用是纯个人学习研究项目，不涉及任何商业行为，也不提供、不分发任何 ROM / 游戏文件。" +
                    "列表里的卡带由你自己从链接下载，请自行确认有权使用。",
            )
            Paragraph(
                "ROM 只在打开一局时下载到缓存，退出这一局就删除；本机留下的只有你自己的存档。" +
                    "应用本身不含任何游戏数据。",
            )
            Paragraph(
                "本项目与任天堂（Nintendo）没有任何关联，未获其授权、认可或赞助；Nintendo、NES 以及游戏名称" +
                    "均为各自所有者的商标，此处仅用于说明代码功能。界面模仿的是九十年代游戏机的外观，不代表任何厂商。",
            )
            Paragraph(
                "仓库、Web UI、iOS 应用与本应用适用同一份声明，完整内容见仓库中的 DISCLAIMER.md。",
            )
            Paragraph("代码按 MIT 许可发布，仅供学习，不提供任何担保。")
            Spacer(Modifier.height(10.dp))
            RetroLabel("BACK 返回", color = Retro.Ash, size = 12, monospace = true)
        }
    }
}

@Composable
private fun Paragraph(text: String) {
    RetroLabel(
        text = text,
        color = Retro.Cream,
        size = 14,
        maxLines = 6,
        modifier = Modifier.width(820.dp).padding(bottom = 12.dp),
    )
}
